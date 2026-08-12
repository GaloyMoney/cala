use std::collections::{HashMap, HashSet, VecDeque};

use crate::primitives::{AccountId, AccountSetId};

use super::error::AccountSetError;

/// Maximum depth (in set->set edges) of any root-to-leaf membership
/// chain. Rejecting edges past this bound keeps the read-time ancestor
/// walk cheap and terminating. Real hierarchies are <=10 deep; 16 leaves
/// headroom.
pub(super) const MAX_MEMBERSHIP_DEPTH: i32 = 16;

/// Count paths from each account's proposed and existing direct memberships
/// through the supplied parent graph. Returns `None` when the graph view does
/// not know a traversed set, allowing the cache adapter to fall back to SQL.
pub(super) fn has_duplicate_account_membership_paths<'a>(
    new_set_ids: &[AccountSetId],
    new_account_ids: &[AccountId],
    existing_seeds: &[(AccountId, AccountSetId)],
    is_known: impl Fn(&AccountSetId) -> bool,
    parents_of: impl Fn(&AccountSetId) -> &'a [AccountSetId],
) -> Option<bool> {
    let mut per_account: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
    for (set_id, account_id) in new_set_ids.iter().zip(new_account_ids) {
        per_account.entry(*account_id).or_default().push(*set_id);
    }
    for (account_id, set_id) in existing_seeds {
        per_account.entry(*account_id).or_default().push(*set_id);
    }

    for seeds in per_account.into_values() {
        let mut path_counts: HashMap<AccountSetId, u32> = HashMap::new();
        let mut queue: VecDeque<AccountSetId> = seeds.into();
        while let Some(set_id) = queue.pop_front() {
            if !is_known(&set_id) {
                return None;
            }
            let count = path_counts.entry(set_id).or_default();
            *count += 1;
            if *count > 1 {
                return Some(true);
            }
            queue.extend(parents_of(&set_id));
        }
    }
    Some(false)
}

pub(super) fn validate_set_memberships(
    existing_edges: &[(AccountSetId, AccountSetId)],
    proposed_edges: &[(AccountSetId, AccountSetId)],
    account_members: &[(AccountSetId, AccountId)],
) -> Result<(), AccountSetError> {
    let mut nodes = HashSet::new();
    let mut adjacency: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
    let mut indegree: HashMap<AccountSetId, usize> = HashMap::new();

    for (account_set_id, member_account_set_id) in existing_edges.iter().chain(proposed_edges) {
        nodes.insert(*account_set_id);
        nodes.insert(*member_account_set_id);
        adjacency
            .entry(*account_set_id)
            .or_default()
            .push(*member_account_set_id);
        *indegree.entry(*member_account_set_id).or_default() += 1;
        indegree.entry(*account_set_id).or_default();
    }
    for (account_set_id, _) in account_members {
        nodes.insert(*account_set_id);
        indegree.entry(*account_set_id).or_default();
    }

    let mut queue: VecDeque<AccountSetId> = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect();
    let mut topological_order = Vec::with_capacity(nodes.len());
    while let Some(account_set_id) = queue.pop_front() {
        topological_order.push(account_set_id);
        if let Some(children) = adjacency.get(&account_set_id) {
            for child in children {
                let degree = indegree
                    .get_mut(child)
                    .expect("every child must have an indegree");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(*child);
                }
            }
        }
    }

    if topological_order.len() != nodes.len() {
        // A cycle must involve at least one proposed edge (the committed
        // graph is a DAG). If it is in the existing graph that is
        // corrupted state; attribute to the first existing edge so the
        // fallback never indexes an empty proposed-edges slice.
        let (account_set_id, member_account_set_id) = proposed_edges
            .iter()
            .find(|(account_set_id, member_account_set_id)| {
                account_set_id == member_account_set_id
                    || graph_has_path(&adjacency, *member_account_set_id, *account_set_id)
            })
            .copied()
            .or_else(|| existing_edges.first().copied())
            .expect("cycle detected in a graph with no edges");
        return Err(AccountSetError::MembershipCycleDetected {
            account_set_id,
            member_account_set_id,
        });
    }

    // In topological order, every parent's ancestor set is complete before
    // its contribution reaches a child. An overlap means the child has two
    // paths to the same ancestor. Hash-set union caps work at O(V^2) in the
    // worst case instead of enumerating exponentially many paths.
    let mut ancestors: HashMap<AccountSetId, HashSet<AccountSetId>> = HashMap::new();
    let mut depth_from_root: HashMap<AccountSetId, i32> = HashMap::new();
    for account_set_id in &topological_order {
        let mut contribution = ancestors.get(account_set_id).cloned().unwrap_or_default();
        contribution.insert(*account_set_id);
        let parent_depth = *depth_from_root.get(account_set_id).unwrap_or(&0);

        if let Some(children) = adjacency.get(account_set_id) {
            for child in children {
                let child_ancestors = ancestors.entry(*child).or_default();
                if !child_ancestors.is_disjoint(&contribution) {
                    return Err(AccountSetError::MemberAlreadyAdded);
                }
                child_ancestors.extend(contribution.iter().copied());
                depth_from_root
                    .entry(*child)
                    .and_modify(|depth| *depth = (*depth).max(parent_depth + 1))
                    .or_insert(parent_depth + 1);
            }
        }
    }

    let mut account_paths = HashSet::new();
    for (account_set_id, account_id) in account_members {
        if !account_paths.insert((*account_set_id, *account_id)) {
            return Err(AccountSetError::MemberAlreadyAdded);
        }
        if let Some(containers) = ancestors.get(account_set_id) {
            for container in containers {
                if !account_paths.insert((*container, *account_id)) {
                    return Err(AccountSetError::MemberAlreadyAdded);
                }
            }
        }
    }

    let max_depth = depth_from_root.values().copied().max().unwrap_or(0);
    if max_depth > MAX_MEMBERSHIP_DEPTH {
        let (index, depth) = first_depth_overflow(existing_edges, proposed_edges);
        let (account_set_id, member_account_set_id) = proposed_edges[index];
        return Err(AccountSetError::MembershipDepthExceeded {
            account_set_id,
            member_account_set_id,
            depth,
            max: MAX_MEMBERSHIP_DEPTH,
        });
    }

    Ok(())
}

fn graph_has_path(
    adjacency: &HashMap<AccountSetId, Vec<AccountSetId>>,
    from: AccountSetId,
    to: AccountSetId,
) -> bool {
    let mut pending = vec![from];
    let mut visited = HashSet::new();
    while let Some(current) = pending.pop() {
        if current == to {
            return true;
        }
        if visited.insert(current) {
            pending.extend(adjacency.get(&current).into_iter().flatten().copied());
        }
    }
    false
}

/// Find the first proposed edge whose inclusion makes the *combined*
/// existing-plus-proposed graph exceed `MAX_MEMBERSHIP_DEPTH`. The returned
/// depth is the maximum depth of that combined graph (not only the depth of
/// the chain through the offending edge). The batch enforces a global depth
/// bound so the read-time ancestor walk stays cheap and terminating; the
/// reported `depth` therefore reflects the bound that was exceeded, and the
/// returned index identifies the first edge responsible.
fn first_depth_overflow(
    existing_edges: &[(AccountSetId, AccountSetId)],
    proposed_edges: &[(AccountSetId, AccountSetId)],
) -> (usize, i32) {
    let mut low = 1;
    let mut high = proposed_edges.len();
    while low < high {
        let middle = (low + high) / 2;
        if graph_max_depth(existing_edges, &proposed_edges[..middle]) > MAX_MEMBERSHIP_DEPTH {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    (
        low - 1,
        graph_max_depth(existing_edges, &proposed_edges[..low]),
    )
}

fn graph_max_depth(
    existing_edges: &[(AccountSetId, AccountSetId)],
    proposed_edges: &[(AccountSetId, AccountSetId)],
) -> i32 {
    let mut adjacency: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
    let mut indegree: HashMap<AccountSetId, usize> = HashMap::new();
    for (account_set_id, member_account_set_id) in existing_edges.iter().chain(proposed_edges) {
        adjacency
            .entry(*account_set_id)
            .or_default()
            .push(*member_account_set_id);
        *indegree.entry(*member_account_set_id).or_default() += 1;
        indegree.entry(*account_set_id).or_default();
    }

    let mut queue: VecDeque<AccountSetId> = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect();
    let mut depths = HashMap::new();
    let mut max_depth = 0;
    while let Some(account_set_id) = queue.pop_front() {
        let parent_depth = *depths.get(&account_set_id).unwrap_or(&0);
        if let Some(children) = adjacency.get(&account_set_id) {
            for child in children {
                let child_depth = parent_depth + 1;
                depths
                    .entry(*child)
                    .and_modify(|depth| *depth = (*depth).max(child_depth))
                    .or_insert(child_depth);
                max_depth = max_depth.max(child_depth);
                let degree = indegree
                    .get_mut(child)
                    .expect("every child must have an indegree");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(*child);
                }
            }
        }
    }
    max_depth
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_ids<const N: usize>() -> [AccountSetId; N] {
        std::array::from_fn(|_| AccountSetId::new())
    }

    #[test]
    fn account_paths_accept_distinct_ancestors() {
        let [left, right] = set_ids();
        let account_id = AccountId::new();
        let known = HashSet::from([left, right]);
        let parents: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();

        assert_eq!(
            has_duplicate_account_membership_paths(
                &[left, right],
                &[account_id, account_id],
                &[],
                |set_id| known.contains(set_id),
                |set_id| parents.get(set_id).map(Vec::as_slice).unwrap_or(&[]),
            ),
            Some(false)
        );
    }

    #[test]
    fn account_paths_reject_a_shared_ancestor() {
        let [root, left, right] = set_ids();
        let account_id = AccountId::new();
        let known = HashSet::from([root, left, right]);
        let parents = HashMap::from([(left, vec![root]), (right, vec![root])]);

        assert_eq!(
            has_duplicate_account_membership_paths(
                &[left, right],
                &[account_id, account_id],
                &[],
                |set_id| known.contains(set_id),
                |set_id| parents.get(set_id).map(Vec::as_slice).unwrap_or(&[]),
            ),
            Some(true)
        );
    }

    #[test]
    fn account_paths_defer_an_unknown_set() {
        let [unknown] = set_ids();

        assert_eq!(
            has_duplicate_account_membership_paths(
                &[unknown],
                &[AccountId::new()],
                &[],
                |_| false,
                |_| &[],
            ),
            None
        );
    }

    #[test]
    fn set_paths_accept_a_valid_combined_tree() {
        let [root, branch, existing_leaf, proposed_leaf] = set_ids();
        let existing = [(root, branch), (branch, existing_leaf)];
        let proposed = [(branch, proposed_leaf)];

        assert!(validate_set_memberships(&existing, &proposed, &[]).is_ok());
    }

    #[test]
    fn set_paths_reject_a_cycle_created_within_the_batch() {
        let [a, b, c] = set_ids();
        let proposed = [(a, b), (b, c), (c, a)];

        assert!(matches!(
            validate_set_memberships(&[], &proposed, &[]),
            Err(AccountSetError::MembershipCycleDetected { .. })
        ));
    }

    #[test]
    fn set_paths_reject_a_duplicate_existing_and_proposed_path() {
        let [root, branch, leaf] = set_ids();
        let existing = [(root, branch), (branch, leaf)];
        let proposed = [(root, leaf)];

        assert!(matches!(
            validate_set_memberships(&existing, &proposed, &[]),
            Err(AccountSetError::MemberAlreadyAdded)
        ));
    }

    #[test]
    fn set_paths_reject_an_account_reachable_twice() {
        let [root, left, right] = set_ids();
        let account_id = AccountId::new();
        let existing = [(root, left), (root, right)];
        let account_members = [(left, account_id), (right, account_id)];

        assert!(matches!(
            validate_set_memberships(&existing, &[], &account_members),
            Err(AccountSetError::MemberAlreadyAdded)
        ));
    }

    #[test]
    fn set_paths_attribute_the_first_depth_overflow() {
        let sets: [AccountSetId; 18] = set_ids();
        let proposed: Vec<_> = sets.windows(2).map(|pair| (pair[0], pair[1])).collect();

        assert!(matches!(
            validate_set_memberships(&[], &proposed, &[]),
            Err(AccountSetError::MembershipDepthExceeded {
                account_set_id,
                member_account_set_id,
                depth: 17,
                max: 16,
            }) if account_set_id == sets[16] && member_account_set_id == sets[17]
        ));
    }
}
