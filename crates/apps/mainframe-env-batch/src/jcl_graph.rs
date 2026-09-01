use std::collections::BTreeSet;

/// One reviewed bounded dependency/cycle utility shared by JCL INCLUDE,
/// procedure expansion, and immutable plan validation.
#[derive(Clone, Debug)]
pub(crate) struct JclDependencyGraph {
    stack: Vec<String>,
    active: BTreeSet<String>,
    visited_edges: usize,
    max_depth: usize,
    max_edges: usize,
}

impl JclDependencyGraph {
    pub(crate) fn new(max_depth: usize, max_edges: usize) -> Self {
        Self {
            stack: Vec::new(),
            active: BTreeSet::new(),
            visited_edges: 0,
            max_depth,
            max_edges,
        }
    }

    pub(crate) fn enter(&mut self, node: String) -> Result<(), JclGraphProblem> {
        self.visited_edges = self
            .visited_edges
            .checked_add(1)
            .ok_or(JclGraphProblem::EdgeLimitExceeded)?;
        if self.visited_edges > self.max_edges {
            return Err(JclGraphProblem::EdgeLimitExceeded);
        }
        if self.stack.len() >= self.max_depth {
            return Err(JclGraphProblem::DepthLimitExceeded);
        }
        if let Some(position) = self.stack.iter().position(|active| active == &node) {
            let mut cycle = self.stack[position..].to_vec();
            cycle.push(node);
            return Err(JclGraphProblem::Cycle(cycle));
        }
        if !self.active.insert(node.clone()) {
            return Err(JclGraphProblem::UnbalancedTraversal);
        }
        self.stack.push(node);
        Ok(())
    }

    pub(crate) fn leave(&mut self, node: &str) -> Result<(), JclGraphProblem> {
        if self.stack.last().map(String::as_str) != Some(node) || !self.active.remove(node) {
            return Err(JclGraphProblem::UnbalancedTraversal);
        }
        self.stack.pop();
        Ok(())
    }

    pub(crate) fn complete(&self) -> Result<(), JclGraphProblem> {
        if self.stack.is_empty() && self.active.is_empty() {
            Ok(())
        } else {
            Err(JclGraphProblem::UnbalancedTraversal)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum JclGraphProblem {
    Cycle(Vec<String>),
    DepthLimitExceeded,
    EdgeLimitExceeded,
    UnbalancedTraversal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_exact_deterministic_cycle_path() {
        let mut graph = JclDependencyGraph::new(8, 8);
        graph.enter("include:A".into()).unwrap();
        graph.enter("include:B".into()).unwrap();
        assert_eq!(
            graph.enter("include:A".into()),
            Err(JclGraphProblem::Cycle(vec![
                "include:A".into(),
                "include:B".into(),
                "include:A".into()
            ]))
        );
        graph.leave("include:B").unwrap();
        graph.leave("include:A").unwrap();
        graph.complete().unwrap();
    }

    #[test]
    fn depth_and_edge_bounds_fail_before_graph_growth() {
        let mut depth = JclDependencyGraph::new(1, 8);
        depth.enter("procedure:A".into()).unwrap();
        assert_eq!(
            depth.enter("procedure:B".into()),
            Err(JclGraphProblem::DepthLimitExceeded)
        );
        let mut edges = JclDependencyGraph::new(8, 1);
        edges.enter("include:A".into()).unwrap();
        edges.leave("include:A").unwrap();
        assert_eq!(
            edges.enter("include:B".into()),
            Err(JclGraphProblem::EdgeLimitExceeded)
        );
    }
}
