//! The tests of `dependency_graph`.

use std::collections::HashSet;

use samplekit::core::dependency_graph::{Change, DependencyError, DependencyGraph, Node};
use samplekit::core::identifier::Identifier;

fn n(name: &str) -> Node {
    Node::named(Identifier::new(name).unwrap())
}

fn c(table: &str, column: &str) -> Node {
    Node::column(
        Identifier::new(table).unwrap(),
        Identifier::new(column).unwrap(),
    )
}

/// `ductility` from `diameter` and `volume`, `roughness` from `ductility`, `rating` from
/// `roughness`. `label` hangs off nothing.
fn chain() -> DependencyGraph {
    let mut graph = DependencyGraph::new();
    graph
        .declare(&n("ductility"), &[n("diameter"), n("volume")])
        .unwrap();
    graph.declare(&n("roughness"), &[n("ductility")]).unwrap();
    graph.declare(&n("rating"), &[n("roughness")]).unwrap();
    graph
}

/// The deterministic generator.
struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

// --------------------------------------------------------------- traversal

#[test]
fn direct_dependents_are_reported() {
    let graph = chain();
    let dependents = graph.dependents_of(&n("diameter"));
    assert_eq!(dependents.len(), 1);
    assert!(dependents.contains(&n("ductility")));
    assert_eq!(graph.inputs_of(&n("ductility")).len(), 2);
}

#[test]
fn invalidation_is_transitive() {
    // A change three carbon upstream reaches the far end.
    let affected = chain().affected_by(&n("diameter"), Change::Value);
    assert_eq!(affected, [n("ductility"), n("roughness"), n("rating")]);
}

#[test]
fn unrelated_properties_are_untouched() {
    let mut graph = chain();
    graph.declare(&n("colour"), &[n("label")]).unwrap();
    let affected = graph.affected_by(&n("diameter"), Change::Value);
    assert!(!affected.contains(&n("label")));
    assert!(!affected.contains(&n("colour")));
    // And the changed node is never in its own list.
    assert!(!affected.contains(&n("diameter")));
}

#[test]
fn each_dependent_appears_once() {
    // A diamond: rating rests on two paths back to diameter.
    let mut graph = DependencyGraph::new();
    graph.declare(&n("left"), &[n("diameter")]).unwrap();
    graph.declare(&n("right"), &[n("diameter")]).unwrap();
    graph
        .declare(&n("rating"), &[n("left"), n("right")])
        .unwrap();
    let affected = graph.affected_by(&n("diameter"), Change::Value);
    assert_eq!(affected, [n("left"), n("right"), n("rating")]);
    let unique: HashSet<&Node> = affected.iter().collect();
    assert_eq!(unique.len(), affected.len());
}

#[test]
fn traversal_order_is_deterministic() {
    // The same graph, declared in two orders, traverses identically.
    let mut one = DependencyGraph::new();
    one.declare(&n("zeta"), &[n("diameter")]).unwrap();
    one.declare(&n("alpha"), &[n("diameter")]).unwrap();
    one.declare(&n("mu"), &[n("diameter")]).unwrap();

    let mut other = DependencyGraph::new();
    other.declare(&n("mu"), &[n("diameter")]).unwrap();
    other.declare(&n("alpha"), &[n("diameter")]).unwrap();
    other.declare(&n("zeta"), &[n("diameter")]).unwrap();

    let expected = [n("alpha"), n("mu"), n("zeta")];
    assert_eq!(one.affected_by(&n("diameter"), Change::Value), expected);
    assert_eq!(other.affected_by(&n("diameter"), Change::Value), expected);
    // And twice from one graph, in case anything iterated a hash map.
    assert_eq!(
        one.affected_by(&n("diameter"), Change::Value),
        one.affected_by(&n("diameter"), Change::Value)
    );
}

#[test]
fn node_order_follows_the_declaration() {
    // Named before Column; then by identifier, and by (table, column).
    assert!(n("zzz") < c("aaa", "aaa"));
    assert!(n("a") < n("b"));
    assert!(c("a", "z") < c("b", "a"));
    assert!(c("a", "a") < c("a", "b"));

    // And the traversal breaks ties by it rather than by whatever it read first.
    let mut graph = DependencyGraph::new();
    graph
        .declare(&c("preheating", "rigidity"), &[n("diameter")])
        .unwrap();
    graph.declare(&n("ductility"), &[n("diameter")]).unwrap();
    assert_eq!(
        graph.affected_by(&n("diameter"), Change::Value),
        [n("ductility"), c("preheating", "rigidity")]
    );
}

#[test]
fn presentation_change_affects_nothing() {
    // Empty even on a deep graph: a unit correction is not a re-derivation.
    assert!(
        chain()
            .affected_by(&n("diameter"), Change::Presentation)
            .is_empty()
    );
}

#[test]
fn unknown_name_has_no_dependents() {
    // Not an error: quantities are created and removed independently.
    let graph = chain();
    assert!(graph.dependents_of(&n("never_declared")).is_empty());
    assert!(graph.inputs_of(&n("never_declared")).is_empty());
    assert!(
        graph
            .affected_by(&n("never_declared"), Change::Value)
            .is_empty()
    );
}

// -------------------------------------------------------------- declaration

#[test]
fn direct_cycle_is_rejected() {
    let mut graph = DependencyGraph::new();
    graph.declare(&n("b"), &[n("a")]).unwrap();
    let error = graph.declare(&n("a"), &[n("b")]).unwrap_err();
    assert_eq!(
        error,
        DependencyError::Cycle {
            path: vec![n("a"), n("b"), n("a")]
        }
    );
    // And the message draws it rather than saying only "cycle detected".
    assert!(error.to_string().contains('\u{2192}'), "{error}");
}

#[test]
fn indirect_cycle_is_rejected() {
    // Four nodes, and the error names all four in order.
    let mut graph = DependencyGraph::new();
    graph.declare(&n("b"), &[n("a")]).unwrap();
    graph.declare(&n("c"), &[n("b")]).unwrap();
    graph.declare(&n("d"), &[n("c")]).unwrap();
    let error = graph.declare(&n("a"), &[n("d")]).unwrap_err();
    assert_eq!(
        error,
        DependencyError::Cycle {
            path: vec![n("a"), n("d"), n("c"), n("b"), n("a")]
        }
    );
}

#[test]
fn self_dependency_is_rejected() {
    // Its own variant: the cause is different and the message says so.
    let mut graph = DependencyGraph::new();
    let error = graph
        .declare(&n("ductility"), &[n("diameter"), n("ductility")])
        .unwrap_err();
    assert_eq!(
        error,
        DependencyError::SelfDependency {
            node: n("ductility")
        }
    );
    assert!(graph.is_empty());
}

#[test]
fn a_cycle_can_run_through_a_column() {
    // A property, a column and back, refused with all three in the path.
    let mut graph = DependencyGraph::new();
    graph
        .declare(&c("preheating", "rigidity"), &[n("wear")])
        .unwrap();
    graph
        .declare(&n("mean_rigidity"), &[c("preheating", "rigidity")])
        .unwrap();
    let error = graph
        .declare(&n("wear"), &[n("mean_rigidity")])
        .unwrap_err();
    let DependencyError::Cycle { path } = error else {
        panic!("expected a cycle, got {error:?}");
    };
    assert_eq!(
        path,
        vec![
            n("wear"),
            n("mean_rigidity"),
            c("preheating", "rigidity"),
            n("wear"),
        ]
    );
}

#[test]
fn failed_batch_leaves_no_edges() {
    // Five relationships where the last cycles records none of the first four.
    let mut graph = DependencyGraph::new();
    let batch = vec![
        (n("b"), vec![n("a")]),
        (n("c"), vec![n("b")]),
        (n("d"), vec![n("c")]),
        (n("e"), vec![n("d")]),
        (n("a"), vec![n("e")]),
    ];
    assert!(graph.declare_all(&batch).is_err());
    assert!(graph.is_empty(), "a rejected batch left edges behind");
    for name in ["a", "b", "c", "d", "e"] {
        assert!(graph.inputs_of(&n(name)).is_empty(), "{name}");
        assert!(graph.dependents_of(&n(name)).is_empty(), "{name}");
    }
}

#[test]
fn redeclaring_replaces_inputs() {
    // Declaring twice does not accumulate: re-running a model is idempotent.
    let mut graph = DependencyGraph::new();
    graph
        .declare(&n("ductility"), &[n("diameter"), n("volume")])
        .unwrap();
    graph.declare(&n("ductility"), &[n("diameter")]).unwrap();
    assert_eq!(graph.inputs_of(&n("ductility")).len(), 1);
    assert!(graph.inputs_of(&n("ductility")).contains(&n("diameter")));
    // And the dropped input no longer names it.
    assert!(graph.dependents_of(&n("volume")).is_empty());
}

// -------------------------------------------------------------- maintenance

#[test]
fn removing_a_name_clears_both_directions() {
    let mut graph = chain();
    graph.remove(&n("ductility"));
    assert!(graph.dependents_of(&n("diameter")).is_empty());
    assert!(graph.inputs_of(&n("roughness")).is_empty());
    assert!(graph.inputs_of(&n("ductility")).is_empty());
    assert!(graph.dependents_of(&n("ductility")).is_empty());
    // A change to a former input reaches nothing that went with it.
    assert!(graph.affected_by(&n("diameter"), Change::Value).is_empty());
}

#[test]
fn a_column_and_a_property_can_share_a_name() {
    // Two nodes, and an edge on one leaves the other alone.
    let mut graph = DependencyGraph::new();
    graph.declare(&n("nominal"), &[n("wear")]).unwrap();
    graph
        .declare(&n("measured"), &[c("preheating", "wear")])
        .unwrap();

    assert_eq!(graph.affected_by(&n("wear"), Change::Value), [n("nominal")]);
    assert_eq!(
        graph.affected_by(&c("preheating", "wear"), Change::Value),
        [n("measured")]
    );
    // And removing one leaves the other standing.
    graph.remove(&n("wear"));
    assert_eq!(
        graph.affected_by(&c("preheating", "wear"), Change::Value),
        [n("measured")]
    );
}

#[test]
fn forward_and_reverse_stay_consistent() {
    // A property test over generated declare/remove sequences: the two maps are
    // written in one operation, and nothing may leave them disagreeing.
    let universe: Vec<Node> = (0..6)
        .map(|i| n(&format!("p{i}")))
        .chain((0..3).map(|i| c("preheating", &format!("q{i}"))))
        .collect();
    let mut generator = Generator(0xD00D);
    let mut graph = DependencyGraph::new();

    for _ in 0..400 {
        let pick =
            |g: &mut Generator| universe[(g.next() % universe.len() as u64) as usize].clone();
        if generator.next().is_multiple_of(5) {
            graph.remove(&pick(&mut generator));
        } else {
            let dependent = pick(&mut generator);
            let inputs: Vec<Node> = (0..generator.next() % 3)
                .map(|_| pick(&mut generator))
                .filter(|input| input != &dependent)
                .collect();
            // A rejected declaration must leave the graph exactly as it was.
            let before = graph.clone();
            if graph.declare(&dependent, &inputs).is_err() {
                assert_eq!(graph, before, "a rejected declaration mutated the graph");
            }
        }

        for node in &universe {
            for input in graph.inputs_of(node) {
                assert!(
                    graph.dependents_of(input).contains(node),
                    "{node} names {input} as an input, and {input} does not name it back"
                );
            }
            for dependent in graph.dependents_of(node) {
                assert!(
                    graph.inputs_of(dependent).contains(node),
                    "{dependent} is a dependent of {node}, and does not name it as an input"
                );
            }
        }
    }
}
