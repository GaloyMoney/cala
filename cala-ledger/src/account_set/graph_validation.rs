use std::collections::{HashMap, HashSet, VecDeque};

use crate::primitives::{AccountId, AccountSetId};

use super::error::AccountSetError;

/// Maximum depth (in set->set edges) of any root-to-leaf membership
/// chain. Rejecting edges past this bound keeps the read-time ancestor
/// walk cheap and terminating. Real hierarchies are <=10 deep; 16 leaves
/// headroom.
pub(super) const MAX_MEMBERSHIP_DEPTH: i32 = 16;

/// A directed hierarchy edge: `member_account_set_id` is a direct member of
/// `account_set_id`.
///
/// Both ends are `AccountSetId`, so a bare pair offers no protection against
/// transposing container and member — a swap type-checks and silently inverts
/// the graph. Naming the ends makes the direction explicit at every call site.
/// The field names mirror the `cala_account_set_member_account_sets` columns
/// and the outbox payload, so one vocabulary carries from SQL through the cache
/// into this validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SetMembership {
    pub account_set_id: AccountSetId,
    pub member_account_set_id: AccountSetId,
}

impl From<(AccountSetId, AccountSetId)> for SetMembership {
    fn from((account_set_id, member_account_set_id): (AccountSetId, AccountSetId)) -> Self {
        Self {
            account_set_id,
            member_account_set_id,
        }
    }
}

/// An account's direct membership in a set.
///
/// This replaces the two transposed pair shapes the module previously carried
/// for one concept — `(set, account)` for proposed members and account-member
/// rows, `(account, set)` for probed seeds. A single orientation means code
/// that consumes both sources (the path walk below) can no longer read one of
/// them backwards. Only the canonical `(set, account)` `From` is provided, so
/// the transposed order cannot be converted in by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct AccountMembership {
    pub account_set_id: AccountSetId,
    pub account_id: AccountId,
}

impl From<(AccountSetId, AccountId)> for AccountMembership {
    fn from((account_set_id, account_id): (AccountSetId, AccountId)) -> Self {
        Self {
            account_set_id,
            account_id,
        }
    }
}

/// Count containment paths from each account's proposed and existing direct
/// memberships upward through `parents_of`.
///
/// `parents_of` answers every state the walk distinguishes: `None` for a set
/// the graph view does not know (the caller falls back to SQL), `Some(&[])`
/// for a known root, and `Some(parents)` otherwise. Returns `None` when any
/// traversed set is unknown, `Some(true)` when some account reaches the same
/// set twice, `Some(false)` when every path is unique.
pub(super) fn has_duplicate_account_membership_paths<'a>(
    proposed: &[AccountMembership],
    existing: &[AccountMembership],
    parents_of: impl Fn(&AccountSetId) -> Option<&'a [AccountSetId]>,
) -> Option<bool> {
    let mut per_account: HashMap<AccountId, Vec<AccountSetId>> = HashMap::new();
    for membership in proposed.iter().chain(existing) {
        per_account
            .entry(membership.account_id)
            .or_default()
            .push(membership.account_set_id);
    }

    for seeds in per_account.into_values() {
        let mut path_counts: HashMap<AccountSetId, u32> = HashMap::new();
        let mut queue: VecDeque<AccountSetId> = seeds.into();
        while let Some(account_set_id) = queue.pop_front() {
            let parents = parents_of(&account_set_id)?;
            let count = path_counts.entry(account_set_id).or_default();
            *count += 1;
            if *count > 1 {
                return Some(true);
            }
            queue.extend(parents);
        }
    }
    Some(false)
}

pub(super) fn validate_set_memberships(
    existing_edges: &[SetMembership],
    proposed_edges: &[SetMembership],
    account_members: &[AccountMembership],
) -> Result<(), AccountSetError> {
    let mut nodes = HashSet::new();
    let mut adjacency: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
    let mut indegree: HashMap<AccountSetId, usize> = HashMap::new();

    for edge in existing_edges.iter().chain(proposed_edges) {
        nodes.insert(edge.account_set_id);
        nodes.insert(edge.member_account_set_id);
        adjacency
            .entry(edge.account_set_id)
            .or_default()
            .push(edge.member_account_set_id);
        *indegree.entry(edge.member_account_set_id).or_default() += 1;
        indegree.entry(edge.account_set_id).or_default();
    }
    for membership in account_members {
        nodes.insert(membership.account_set_id);
        indegree.entry(membership.account_set_id).or_default();
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
        let edge = proposed_edges
            .iter()
            .find(|edge| {
                edge.account_set_id == edge.member_account_set_id
                    || graph_has_path(&adjacency, edge.member_account_set_id, edge.account_set_id)
            })
            .copied()
            .or_else(|| existing_edges.first().copied())
            .expect("cycle detected in a graph with no edges");
        return Err(AccountSetError::MembershipCycleDetected {
            account_set_id: edge.account_set_id,
            member_account_set_id: edge.member_account_set_id,
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

    // Every containment an account gains, direct or inherited, must be
    // reached exactly once. `AccountMembership` is the natural set element:
    // a repeated insert *is* a duplicate containment path.
    let mut account_paths = HashSet::new();
    for membership in account_members {
        if !account_paths.insert(*membership) {
            return Err(AccountSetError::MemberAlreadyAdded);
        }
        if let Some(containers) = ancestors.get(&membership.account_set_id) {
            for container in containers {
                if !account_paths.insert(AccountMembership {
                    account_set_id: *container,
                    account_id: membership.account_id,
                }) {
                    return Err(AccountSetError::MemberAlreadyAdded);
                }
            }
        }
    }

    let max_depth = depth_from_root.values().copied().max().unwrap_or(0);
    if max_depth > MAX_MEMBERSHIP_DEPTH {
        let (index, depth) = first_depth_overflow(existing_edges, proposed_edges);
        let edge = proposed_edges[index];
        return Err(AccountSetError::MembershipDepthExceeded {
            account_set_id: edge.account_set_id,
            member_account_set_id: edge.member_account_set_id,
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
    existing_edges: &[SetMembership],
    proposed_edges: &[SetMembership],
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

fn graph_max_depth(existing_edges: &[SetMembership], proposed_edges: &[SetMembership]) -> i32 {
    let mut adjacency: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
    let mut indegree: HashMap<AccountSetId, usize> = HashMap::new();
    for edge in existing_edges.iter().chain(proposed_edges) {
        adjacency
            .entry(edge.account_set_id)
            .or_default()
            .push(edge.member_account_set_id);
        *indegree.entry(edge.member_account_set_id).or_default() += 1;
        indegree.entry(edge.account_set_id).or_default();
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

    fn edge(account_set_id: AccountSetId, member_account_set_id: AccountSetId) -> SetMembership {
        SetMembership {
            account_set_id,
            member_account_set_id,
        }
    }

    fn member(account_set_id: AccountSetId, account_id: AccountId) -> AccountMembership {
        AccountMembership {
            account_set_id,
            account_id,
        }
    }

    #[test]
    fn account_paths_accept_distinct_ancestors() {
        let [left, right] = set_ids();
        let account_id = AccountId::new();
        let known = HashSet::from([left, right]);
        let parents: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();

        assert_eq!(
            has_duplicate_account_membership_paths(
                &[member(left, account_id), member(right, account_id)],
                &[],
                |account_set_id| known.contains(account_set_id).then(|| parents
                    .get(account_set_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])),
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
                &[member(left, account_id), member(right, account_id)],
                &[],
                |account_set_id| known.contains(account_set_id).then(|| parents
                    .get(account_set_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])),
            ),
            Some(true)
        );
    }

    #[test]
    fn account_paths_defer_an_unknown_set() {
        let [unknown] = set_ids();

        assert_eq!(
            has_duplicate_account_membership_paths(
                &[member(unknown, AccountId::new())],
                &[],
                |_| None,
            ),
            None
        );
    }

    #[test]
    fn set_paths_accept_a_valid_combined_tree() {
        let [root, branch, existing_leaf, proposed_leaf] = set_ids();
        let existing = [edge(root, branch), edge(branch, existing_leaf)];
        let proposed = [edge(branch, proposed_leaf)];

        assert!(validate_set_memberships(&existing, &proposed, &[]).is_ok());
    }

    #[test]
    fn set_paths_reject_a_cycle_created_within_the_batch() {
        let [a, b, c] = set_ids();
        let proposed = [edge(a, b), edge(b, c), edge(c, a)];

        assert!(matches!(
            validate_set_memberships(&[], &proposed, &[]),
            Err(AccountSetError::MembershipCycleDetected { .. })
        ));
    }

    #[test]
    fn set_paths_reject_a_duplicate_existing_and_proposed_path() {
        let [root, branch, leaf] = set_ids();
        let existing = [edge(root, branch), edge(branch, leaf)];
        let proposed = [edge(root, leaf)];

        assert!(matches!(
            validate_set_memberships(&existing, &proposed, &[]),
            Err(AccountSetError::MemberAlreadyAdded)
        ));
    }

    #[test]
    fn set_paths_reject_an_account_reachable_twice() {
        let [root, left, right] = set_ids();
        let account_id = AccountId::new();
        let existing = [edge(root, left), edge(root, right)];
        let account_members = [member(left, account_id), member(right, account_id)];

        assert!(matches!(
            validate_set_memberships(&existing, &[], &account_members),
            Err(AccountSetError::MemberAlreadyAdded)
        ));
    }

    #[test]
    fn set_paths_attribute_the_first_depth_overflow() {
        let sets: [AccountSetId; 18] = set_ids();
        let proposed: Vec<_> = sets.windows(2).map(|pair| edge(pair[0], pair[1])).collect();

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
