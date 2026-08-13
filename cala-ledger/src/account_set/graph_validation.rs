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

/// The combined existing-plus-proposed set graph, indexed once.
///
/// Nodes are exactly the keys of `indegree`: every edge endpoint gets an entry
/// (the member's is incremented, the container's defaulted), and isolated sets
/// are added explicitly. That makes a separate node set redundant, so the cycle
/// test is `order.len() != node_count()`.
struct SetDag {
    adjacency: HashMap<AccountSetId, Vec<AccountSetId>>,
    indegree: HashMap<AccountSetId, usize>,
}

/// One Kahn pass over a [`SetDag`], yielding both products the validator needs:
/// the topological order and each node's longest-path depth from a root.
///
/// These were previously two separate walks — one ordering, one measuring
/// depth — computing the same recurrence with the same `max` merge. Producing
/// them together is what keeps a single traversal implementation in the module.
struct Traversal {
    order: Vec<AccountSetId>,
    depths: HashMap<AccountSetId, i32>,
}

impl Traversal {
    fn max_depth(&self) -> i32 {
        self.depths.values().copied().max().unwrap_or(0)
    }
}

impl SetDag {
    fn new<'a>(edges: impl IntoIterator<Item = &'a SetMembership>) -> Self {
        let mut adjacency: HashMap<AccountSetId, Vec<AccountSetId>> = HashMap::new();
        let mut indegree: HashMap<AccountSetId, usize> = HashMap::new();
        for edge in edges {
            adjacency
                .entry(edge.account_set_id)
                .or_default()
                .push(edge.member_account_set_id);
            *indegree.entry(edge.member_account_set_id).or_default() += 1;
            indegree.entry(edge.account_set_id).or_default();
        }
        Self {
            adjacency,
            indegree,
        }
    }

    /// Register a set that carries no edges of its own, so it still counts as
    /// a node for the cycle test.
    fn add_isolated(&mut self, account_set_id: AccountSetId) {
        self.indegree.entry(account_set_id).or_default();
    }

    fn node_count(&self) -> usize {
        self.indegree.len()
    }

    fn children(&self, account_set_id: &AccountSetId) -> &[AccountSetId] {
        self.adjacency
            .get(account_set_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Kahn's algorithm. `order` is shorter than [`Self::node_count`] exactly
    /// when the graph contains a cycle (the nodes on it never reach indegree 0).
    fn traverse(&self) -> Traversal {
        // Cloned because the walk consumes indegrees and `&self` may be
        // traversed more than once; O(V), dominated by the O(V + E) walk.
        let mut remaining = self.indegree.clone();
        let mut queue: VecDeque<AccountSetId> = remaining
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        let mut order = Vec::with_capacity(remaining.len());
        let mut depths: HashMap<AccountSetId, i32> = HashMap::new();

        while let Some(account_set_id) = queue.pop_front() {
            order.push(account_set_id);
            let parent_depth = *depths.get(&account_set_id).unwrap_or(&0);
            for child in self.children(&account_set_id) {
                depths
                    .entry(*child)
                    .and_modify(|depth| *depth = (*depth).max(parent_depth + 1))
                    .or_insert(parent_depth + 1);
                let degree = remaining
                    .get_mut(child)
                    .expect("every child must have an indegree");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(*child);
                }
            }
        }
        Traversal { order, depths }
    }

    fn has_path(&self, from: AccountSetId, to: AccountSetId) -> bool {
        let mut pending = vec![from];
        let mut visited = HashSet::new();
        while let Some(current) = pending.pop() {
            if current == to {
                return true;
            }
            if visited.insert(current) {
                pending.extend(self.children(&current));
            }
        }
        false
    }
}

pub(super) fn validate_set_memberships(
    existing_edges: &[SetMembership],
    proposed_edges: &[SetMembership],
    account_members: &[AccountMembership],
) -> Result<(), AccountSetError> {
    let mut dag = SetDag::new(existing_edges.iter().chain(proposed_edges));
    for membership in account_members {
        dag.add_isolated(membership.account_set_id);
    }
    let traversal = dag.traverse();

    if traversal.order.len() != dag.node_count() {
        // A cycle must involve at least one proposed edge (the committed
        // graph is a DAG). If it is in the existing graph that is
        // corrupted state; attribute to the first existing edge so the
        // fallback never indexes an empty proposed-edges slice.
        let edge = proposed_edges
            .iter()
            .find(|edge| {
                edge.account_set_id == edge.member_account_set_id
                    || dag.has_path(edge.member_account_set_id, edge.account_set_id)
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
    for account_set_id in &traversal.order {
        let mut contribution = ancestors.get(account_set_id).cloned().unwrap_or_default();
        contribution.insert(*account_set_id);

        for child in dag.children(account_set_id) {
            let child_ancestors = ancestors.entry(*child).or_default();
            if !child_ancestors.is_disjoint(&contribution) {
                return Err(AccountSetError::MemberAlreadyAdded);
            }
            child_ancestors.extend(contribution.iter().copied());
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

    if traversal.max_depth() > MAX_MEMBERSHIP_DEPTH {
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

/// Find the first proposed edge whose inclusion makes the *combined*
/// existing-plus-proposed graph exceed `MAX_MEMBERSHIP_DEPTH`. The returned
/// depth is the maximum depth of that combined graph (not only the depth of
/// the chain through the offending edge). The batch enforces a global depth
/// bound so the read-time ancestor walk stays cheap and terminating; the
/// reported `depth` therefore reflects the bound that was exceeded, and the
/// returned index identifies the first edge responsible.
///
/// Each probe rebuilds the graph for its prefix, so this is O(E log P) — but it
/// runs only once the batch is already being rejected, never on the accept
/// path. Depth is monotone in the prefix length, which is what makes the binary
/// search valid; maintaining depths incrementally instead would be O(P(V + E)),
/// worse than this for exactly the large batches that would motivate it.
fn first_depth_overflow(
    existing_edges: &[SetMembership],
    proposed_edges: &[SetMembership],
) -> (usize, i32) {
    let prefix_depth = |take: usize| {
        SetDag::new(existing_edges.iter().chain(&proposed_edges[..take]))
            .traverse()
            .max_depth()
    };
    let mut low = 1;
    let mut high = proposed_edges.len();
    while low < high {
        let middle = (low + high) / 2;
        if prefix_depth(middle) > MAX_MEMBERSHIP_DEPTH {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    (low - 1, prefix_depth(low))
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
    fn dag_orders_parents_before_members() {
        let [root, branch, leaf] = set_ids();
        let dag = SetDag::new(&[edge(root, branch), edge(branch, leaf)]);

        let order = dag.traverse().order;

        let position = |id| order.iter().position(|other| *other == id).unwrap();
        assert_eq!(order.len(), dag.node_count());
        assert!(position(root) < position(branch));
        assert!(position(branch) < position(leaf));
    }

    #[test]
    fn dag_counts_isolated_sets_as_nodes() {
        let [root, branch, lone] = set_ids();
        let mut dag = SetDag::new(&[edge(root, branch)]);
        assert_eq!(dag.node_count(), 2);

        dag.add_isolated(lone);

        assert_eq!(dag.node_count(), 3);
        assert_eq!(dag.traverse().order.len(), 3);
    }

    #[test]
    fn dag_leaves_a_cycles_nodes_out_of_the_order() {
        let [a, b, c] = set_ids();
        let dag = SetDag::new(&[edge(a, b), edge(b, c), edge(c, a)]);

        // Every node on the cycle keeps a nonzero indegree, so none is emitted.
        assert!(dag.traverse().order.len() < dag.node_count());
    }

    #[test]
    fn dag_measures_the_longest_path_not_the_shortest() {
        // root -> branch -> leaf is longer than the direct root -> leaf edge.
        let [root, branch, leaf] = set_ids();
        let dag = SetDag::new(&[edge(root, branch), edge(branch, leaf), edge(root, leaf)]);

        assert_eq!(dag.traverse().max_depth(), 2);
    }

    #[test]
    fn dag_max_depth_of_an_empty_graph_is_zero() {
        assert_eq!(SetDag::new(&[]).traverse().max_depth(), 0);
    }

    #[test]
    fn dag_finds_paths_only_downward() {
        let [root, branch, leaf] = set_ids();
        let dag = SetDag::new(&[edge(root, branch), edge(branch, leaf)]);

        assert!(dag.has_path(root, leaf));
        assert!(!dag.has_path(leaf, root));
    }

    #[test]
    fn dag_traversal_is_repeatable() {
        // `traverse` takes &self and must not consume the indegrees it walks.
        let [root, branch] = set_ids();
        let dag = SetDag::new(&[edge(root, branch)]);

        assert_eq!(dag.traverse().order, dag.traverse().order);
        assert_eq!(dag.traverse().max_depth(), 1);
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
