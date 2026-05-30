//! Pure BFS helpers over the location-connection graph, used by Hunter
//! movement (#128, Rules Reference p.12 "shortest path towards the
//! nearest investigator").

use std::collections::{BTreeMap, VecDeque};

use crate::state::{GameState, LocationId};

/// Breadth-first distance (edge count) from `from` to `to` over the
/// location-connection graph. `Some(0)` when `from == to`; `None` when
/// `to` is unreachable. Connections are treated as given in
/// `Location.connections` (the engine maintains them bidirectionally,
/// but BFS does not assume that).
#[allow(dead_code)] // used by hunter movement (Task 6, #128)
pub(crate) fn bfs_distance(state: &GameState, from: LocationId, to: LocationId) -> Option<u32> {
    if from == to {
        return Some(0);
    }
    let mut seen: BTreeMap<LocationId, u32> = BTreeMap::new();
    seen.insert(from, 0);
    let mut queue: VecDeque<LocationId> = VecDeque::new();
    queue.push_back(from);
    while let Some(cur) = queue.pop_front() {
        let dist = seen[&cur];
        let Some(loc) = state.locations.get(&cur) else {
            continue;
        };
        for &next in &loc.connections {
            if next == to {
                return Some(dist + 1);
            }
            if let std::collections::btree_map::Entry::Vacant(e) = seen.entry(next) {
                e.insert(dist + 1);
                queue.push_back(next);
            }
        }
    }
    None
}

/// Every neighbor of `from` that lies on *a* shortest path to `to`,
/// i.e. each connected location `n` with
/// `bfs_distance(n, to) == bfs_distance(from, to) - 1`. Empty when `to`
/// is unreachable or `from == to` (no step needed). Result order
/// follows `from`'s `connections` order; callers that need determinism
/// across that should sort.
#[allow(dead_code)] // used by hunter movement (Task 6, #128)
pub(crate) fn shortest_first_steps(
    state: &GameState,
    from: LocationId,
    to: LocationId,
) -> Vec<LocationId> {
    let Some(total) = bfs_distance(state, from, to) else {
        return Vec::new();
    };
    if total == 0 {
        return Vec::new();
    }
    let Some(loc) = state.locations.get(&from) else {
        return Vec::new();
    };
    loc.connections
        .iter()
        .copied()
        .filter(|&n| bfs_distance(state, n, to) == Some(total - 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{LocationId, Phase};
    use crate::test_support::{test_location, TestGame};

    /// Build a diamond: A(1) connects to B(2) and C(3); both connect to
    /// D(4). Bidirectional edges.
    fn diamond() -> crate::state::GameState {
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        let mut c = test_location(3, "C");
        let mut d = test_location(4, "D");
        a.connections = vec![LocationId(2), LocationId(3)];
        b.connections = vec![LocationId(1), LocationId(4)];
        c.connections = vec![LocationId(1), LocationId(4)];
        d.connections = vec![LocationId(2), LocationId(3)];
        TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_location(c)
            .with_location(d)
            .build()
    }

    #[test]
    fn distance_same_location_is_zero() {
        let s = diamond();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(1)), Some(0));
    }

    #[test]
    fn distance_adjacent_is_one() {
        let s = diamond();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(2)), Some(1));
    }

    #[test]
    fn distance_across_diamond_is_two() {
        let s = diamond();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(4)), Some(2));
    }

    #[test]
    fn distance_unreachable_is_none() {
        let mut a = test_location(1, "A");
        let island = test_location(9, "Island");
        a.connections = vec![];
        let s = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(island)
            .build();
        assert_eq!(bfs_distance(&s, LocationId(1), LocationId(9)), None);
    }

    #[test]
    fn first_steps_single_when_one_shortest_path() {
        // Linear A-B-D (remove C). Only step toward D from A is B.
        let mut a = test_location(1, "A");
        let mut b = test_location(2, "B");
        let mut d = test_location(4, "D");
        a.connections = vec![LocationId(2)];
        b.connections = vec![LocationId(1), LocationId(4)];
        d.connections = vec![LocationId(2)];
        let s = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(b)
            .with_location(d)
            .build();
        assert_eq!(
            shortest_first_steps(&s, LocationId(1), LocationId(4)),
            vec![LocationId(2)]
        );
    }

    #[test]
    fn first_steps_both_when_two_equal_paths() {
        // Diamond: from A to D, both B and C are on a shortest path.
        let s = diamond();
        let mut steps = shortest_first_steps(&s, LocationId(1), LocationId(4));
        steps.sort();
        assert_eq!(steps, vec![LocationId(2), LocationId(3)]);
    }

    #[test]
    fn first_steps_empty_when_unreachable() {
        let mut a = test_location(1, "A");
        let island = test_location(9, "Island");
        a.connections = vec![];
        let s = TestGame::new()
            .with_phase(Phase::Enemy)
            .with_location(a)
            .with_location(island)
            .build();
        assert!(shortest_first_steps(&s, LocationId(1), LocationId(9)).is_empty());
    }

    #[test]
    fn first_steps_empty_when_already_at_target() {
        let s = diamond();
        assert!(shortest_first_steps(&s, LocationId(1), LocationId(1)).is_empty());
    }
}
