//! Declared derivations, and what becomes stale when one of them changes.
//!
//! It stores nodes, not quantities. It never reads a value, never runs a
//! formula, and never decides when recomputation happens.
//!

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::OnceLock;

use crate::core::identifier::Identifier;

/// One end of a derivation.
///
/// **The declaration order is the node order**, used to break ties in a
/// traversal: `Named` before `Column`, then by identifier. Written down rather
/// than left to the implementation, because two conforming implementations
/// obeying a rule described only as *by name* would produce two different
/// invalidation reports.
///
/// A node is at most two already-validated identifiers. An index value never
/// enters one: a column's derivation is declared once and applied to every row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Node {
    /// A property or an attribute. This module neither knows nor needs to know
    /// which: they share one namespace, so a name resolves to exactly one.
    Named(Identifier),
    /// Every cell of one column.
    Column {
        table: Identifier,
        column: Identifier,
    },
}

impl Node {
    pub fn named(name: Identifier) -> Node {
        Node::Named(name)
    }

    pub fn column(table: Identifier, column: Identifier) -> Node {
        Node::Column { table, column }
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Named(name) => write!(f, "{name}"),
            Node::Column { table, column } => write!(f, "{table}.{column}"),
        }
    }
}

/// What kind of change occurred.
///
/// `Presentation` propagates to nothing — a unit or a precision alters how a
/// quantity is written, never what it is. Making this an explicit parameter
/// rather than an assumption at the call site is what keeps a label correction
/// from invalidating a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Change {
    Value,
    Presentation,
}

/// The declared derivations of one sample, in both directions.
///
/// Declaration is natural forwards ("abv depends on og and fg");
/// invalidation traverses backwards ("og changed — who cares?"). Deriving one
/// from the other on each change is O(edges) per invalidation, which is the
/// wrong cost for the operation that runs most often.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DependencyGraph {
    edges: HashMap<Node, HashSet<Node>>,
    reverse: HashMap<Node, HashSet<Node>>,
}

fn no_nodes() -> &'static HashSet<Node> {
    static EMPTY: OnceLock<HashSet<Node>> = OnceLock::new();
    EMPTY.get_or_init(HashSet::new)
}

impl DependencyGraph {
    pub fn new() -> DependencyGraph {
        DependencyGraph::default()
    }

    /// Declare that `dependent` is computed from `inputs`.
    ///
    /// Declaring a dependent that already has inputs **replaces** them rather
    /// than adding to them, so that re-running a model definition is
    /// idempotent.
    pub fn declare(&mut self, dependent: &Node, inputs: &[Node]) -> Result<(), DependencyError> {
        self.declare_all(std::slice::from_ref(&(dependent.clone(), inputs.to_vec())))
    }

    /// Declare a batch, **transactionally**.
    ///
    /// A batch that introduces a cycle at its fifth relationship leaves none of
    /// the first four behind, because a half-applied dependency graph is worse
    /// than no graph: it invalidates some things and not others, and the gap is
    /// invisible.
    pub fn declare_all(
        &mut self,
        relationships: &[(Node, Vec<Node>)],
    ) -> Result<(), DependencyError> {
        // Its own variant rather than a one-element cycle: the cause is
        // different — usually a copy-paste in a model definition — and the
        // message can say so directly.
        for (dependent, inputs) in relationships {
            if inputs.contains(dependent) {
                return Err(DependencyError::SelfDependency {
                    node: dependent.clone(),
                });
            }
        }

        // Validated on a candidate, then committed. A later relationship in one
        // batch can close a cycle only in combination with an earlier one, so
        // every edge has to be present before any is checked.
        let mut candidate = self.clone();
        for (dependent, inputs) in relationships {
            candidate.apply(dependent, inputs);
        }
        for (dependent, _) in relationships {
            if let Some(path) = candidate.cycle_through(dependent) {
                return Err(DependencyError::Cycle { path });
            }
        }

        *self = candidate;
        Ok(())
    }

    fn apply(&mut self, dependent: &Node, inputs: &[Node]) {
        for previous in self.edges.remove(dependent).unwrap_or_default() {
            if let Some(dependents) = self.reverse.get_mut(&previous) {
                dependents.remove(dependent);
            }
        }
        for input in inputs {
            self.edges
                .entry(dependent.clone())
                .or_default()
                .insert(input.clone());
            self.reverse
                .entry(input.clone())
                .or_default()
                .insert(dependent.clone());
        }
        self.edges.entry(dependent.clone()).or_default();
    }

    /// The path of a cycle reachable from `start`, closing back on it.
    ///
    /// Reported as `plato → haze → rating → plato`: a message saying
    /// only "cycle detected" sends the author to read every declaration they
    /// wrote; the path sends them to the one that is wrong.
    fn cycle_through(&self, start: &Node) -> Option<Vec<Node>> {
        let mut path = vec![start.clone()];
        let mut seen = HashSet::new();
        self.walk_for_cycle(start, start, &mut path, &mut seen)
            .then_some(path)
    }

    fn walk_for_cycle(
        &self,
        current: &Node,
        start: &Node,
        path: &mut Vec<Node>,
        seen: &mut HashSet<Node>,
    ) -> bool {
        for input in sorted(self.inputs_of(current)) {
            if &input == start {
                path.push(input);
                return true;
            }
            if !seen.insert(input.clone()) {
                continue;
            }
            path.push(input.clone());
            if self.walk_for_cycle(&input, start, path, seen) {
                return true;
            }
            path.pop();
        }
        false
    }

    /// What this node is computed from. An unknown node has no edges, which is
    /// correct rather than lenient: quantities are created and removed
    /// independently of the graph.
    pub fn inputs_of(&self, node: &Node) -> &HashSet<Node> {
        self.edges.get(node).unwrap_or_else(|| no_nodes())
    }

    /// What is computed from this node, one hop.
    pub fn dependents_of(&self, node: &Node) -> &HashSet<Node> {
        self.reverse.get(node).unwrap_or_else(|| no_nodes())
    }

    /// Everything transitively reachable from a change, each exactly once,
    /// excluding the changed node itself.
    ///
    /// Breadth-first, ties broken by [`Node`]'s declaration order. Determinism
    /// matters beyond tidiness: invalidation order appears in diagnostics, and a
    /// non-deterministic list makes a report irreproducible between runs.
    pub fn affected_by(&self, node: &Node, change: Change) -> Vec<Node> {
        if change == Change::Presentation {
            return Vec::new();
        }
        let mut affected = Vec::new();
        let mut seen = HashSet::new();
        let mut queue: VecDeque<Node> = VecDeque::new();
        for dependent in sorted(self.dependents_of(node)) {
            if seen.insert(dependent.clone()) {
                queue.push_back(dependent);
            }
        }
        while let Some(current) = queue.pop_front() {
            for dependent in sorted(self.dependents_of(&current)) {
                if seen.insert(dependent.clone()) {
                    queue.push_back(dependent);
                }
            }
            affected.push(current);
        }
        affected
    }

    /// Delete every edge touching this node, in both directions.
    ///
    /// A dangling edge would make `affected_by` name something that no longer
    /// exists, which a caller would then fail to find.
    ///
    /// Removing a **table** is removing one `Column` node per column, which the
    /// caller does because it is the one that knows the columns.
    pub fn remove(&mut self, node: &Node) {
        for input in self.edges.remove(node).unwrap_or_default() {
            if let Some(dependents) = self.reverse.get_mut(&input) {
                dependents.remove(node);
            }
        }
        for dependent in self.reverse.remove(node).unwrap_or_default() {
            if let Some(inputs) = self.edges.get_mut(&dependent) {
                inputs.remove(node);
            }
        }
    }

    pub fn clear(&mut self) {
        self.edges.clear();
        self.reverse.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.edges.values().all(HashSet::is_empty)
    }
}

fn sorted(nodes: &HashSet<Node>) -> Vec<Node> {
    let mut ordered: Vec<Node> = nodes.iter().cloned().collect();
    ordered.sort();
    ordered
}

/// The two ways a declaration is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyError {
    Cycle { path: Vec<Node> },
    SelfDependency { node: Node },
}

impl fmt::Display for DependencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DependencyError::Cycle { path } => {
                let drawn: Vec<String> = path.iter().map(Node::to_string).collect();
                write!(
                    f,
                    "these derivations form a cycle: {}. One of them is wrong",
                    drawn.join(" \u{2192} ")
                )
            }
            DependencyError::SelfDependency { node } => write!(
                f,
                "'{node}' is declared as its own input, which is usually a \
                 copy-paste in a model definition"
            ),
        }
    }
}

impl std::error::Error for DependencyError {}
