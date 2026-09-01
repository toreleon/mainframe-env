use mainframe_env_host_api::HostProblem;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DependencyLimits {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_depth: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DependencyGraph {
    dependencies: BTreeMap<String, BTreeSet<String>>,
}

impl DependencyGraph {
    pub(crate) fn add_node(
        &mut self,
        node: &str,
        limits: DependencyLimits,
    ) -> Result<(), HostProblem> {
        validate_node(node)?;
        if !self.dependencies.contains_key(node) && self.dependencies.len() >= limits.max_nodes {
            return Err(HostProblem::ResourceExhausted);
        }
        self.dependencies.entry(node.into()).or_default();
        Ok(())
    }

    pub(crate) fn add_dependency(
        &mut self,
        dependent: &str,
        authority: &str,
        limits: DependencyLimits,
    ) -> Result<(), HostProblem> {
        self.add_node(dependent, limits)?;
        self.add_node(authority, limits)?;
        if dependent == authority || self.reachable(authority, dependent, limits.max_depth)? {
            return Err(cycle_condition());
        }
        if self.edge_count() >= limits.max_edges
            && !self.dependencies[dependent].contains(authority)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        self.dependencies
            .get_mut(dependent)
            .ok_or(HostProblem::InfrastructureFailure)?
            .insert(authority.into());
        Ok(())
    }

    pub(crate) fn remove_node(&mut self, node: &str) {
        self.dependencies.remove(node);
        for dependencies in self.dependencies.values_mut() {
            dependencies.remove(node);
        }
    }

    pub(crate) fn rename_node(
        &mut self,
        from: &str,
        to: &str,
        limits: DependencyLimits,
    ) -> Result<(), HostProblem> {
        validate_node(to)?;
        if !self.dependencies.contains_key(from) {
            return Err(HostProblem::NotFound);
        }
        if self.dependencies.contains_key(to) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let outgoing = self
            .dependencies
            .remove(from)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.dependencies.insert(to.into(), outgoing);
        for dependencies in self.dependencies.values_mut() {
            if dependencies.remove(from) {
                dependencies.insert(to.into());
            }
        }
        if self.edge_count() > limits.max_edges || self.dependencies.len() > limits.max_nodes {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(())
    }

    pub(crate) fn direct_dependencies(&self, node: &str) -> Vec<String> {
        self.dependencies
            .get(node)
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    pub(crate) fn set_direct_dependencies(
        &mut self,
        node: &str,
        authorities: impl IntoIterator<Item = String>,
        limits: DependencyLimits,
    ) -> Result<(), HostProblem> {
        self.add_node(node, limits)?;
        let previous = self.dependencies.get(node).cloned().unwrap_or_default();
        self.dependencies.insert(node.into(), BTreeSet::new());
        for authority in authorities {
            if let Err(problem) = self.add_dependency(node, &authority, limits) {
                self.dependencies.insert(node.into(), previous);
                return Err(problem);
            }
        }
        Ok(())
    }

    pub(crate) fn invalidation_order(
        &self,
        authority: &str,
        limits: DependencyLimits,
    ) -> Result<Vec<String>, HostProblem> {
        if !self.dependencies.contains_key(authority) {
            return Err(HostProblem::NotFound);
        }
        let mut closure = BTreeSet::from([authority.to_string()]);
        let mut frontier = vec![(authority.to_string(), 0usize)];
        while let Some((current, depth)) = frontier.pop() {
            if depth >= limits.max_depth {
                return Err(HostProblem::ResourceExhausted);
            }
            for dependent in self
                .dependencies
                .iter()
                .filter_map(|(node, dependencies)| dependencies.contains(&current).then_some(node))
            {
                if closure.insert(dependent.clone()) {
                    frontier.push((dependent.clone(), depth + 1));
                }
            }
        }
        let mut pending = closure.clone();
        let mut order = Vec::with_capacity(closure.len());
        while !pending.is_empty() {
            let removable = pending
                .iter()
                .filter(|node| {
                    !pending.iter().any(|candidate| {
                        candidate != *node
                            && self.dependencies[candidate.as_str()].contains(node.as_str())
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if removable.is_empty() {
                return Err(cycle_condition());
            }
            for node in removable {
                pending.remove(&node);
                order.push(node);
            }
        }
        Ok(order)
    }

    fn reachable(&self, from: &str, target: &str, max_depth: usize) -> Result<bool, HostProblem> {
        let mut seen = BTreeSet::new();
        let mut frontier = vec![(from.to_string(), 0usize)];
        while let Some((node, depth)) = frontier.pop() {
            if node == target {
                return Ok(true);
            }
            if !seen.insert(node.clone()) {
                continue;
            }
            if depth >= max_depth {
                return Err(HostProblem::ResourceExhausted);
            }
            frontier.extend(
                self.dependencies
                    .get(&node)
                    .into_iter()
                    .flatten()
                    .cloned()
                    .map(|next| (next, depth + 1)),
            );
        }
        Ok(false)
    }

    fn edge_count(&self) -> usize {
        self.dependencies.values().map(BTreeSet::len).sum()
    }
}

fn validate_node(node: &str) -> Result<(), HostProblem> {
    if node.is_empty() || node.len() > 256 || node.chars().any(char::is_control) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn cycle_condition() -> HostProblem {
    HostProblem::Condition {
        name: "CATCYCLE".into(),
        response: 16,
        response2: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> DependencyLimits {
        DependencyLimits {
            max_nodes: 16,
            max_edges: 32,
            max_depth: 8,
        }
    }

    #[test]
    fn one_graph_orders_alias_path_index_generation_and_migration_dependencies() {
        let mut graph = DependencyGraph::default();
        for (dependent, authority) in [
            ("BASE.AIX", "BASE"),
            ("BASE.PATH", "BASE.AIX"),
            ("BASE.ALIAS", "BASE.PATH"),
            ("BASE.G0001V00", "BASE"),
            ("BASE.MIGRATED", "BASE.G0001V00"),
        ] {
            graph
                .add_dependency(dependent, authority, limits())
                .unwrap();
        }
        assert_eq!(
            graph.invalidation_order("BASE", limits()).unwrap(),
            [
                "BASE.ALIAS",
                "BASE.MIGRATED",
                "BASE.G0001V00",
                "BASE.PATH",
                "BASE.AIX",
                "BASE",
            ]
        );
    }

    #[test]
    fn cycles_and_bounds_fail_without_partial_edges() {
        let mut graph = DependencyGraph::default();
        graph.add_dependency("B", "A", limits()).unwrap();
        graph.add_dependency("C", "B", limits()).unwrap();
        assert!(matches!(
            graph.add_dependency("A", "C", limits()),
            Err(HostProblem::Condition { ref name, response: 16, .. }) if name == "CATCYCLE"
        ));
        assert!(graph.direct_dependencies("A").is_empty());
        assert_eq!(graph.direct_dependencies("C"), ["B"]);
    }

    #[test]
    fn removal_clears_incoming_and_outgoing_edges() {
        let mut graph = DependencyGraph::default();
        graph.add_dependency("B", "A", limits()).unwrap();
        graph.add_dependency("C", "B", limits()).unwrap();
        graph.remove_node("B");
        assert!(graph.direct_dependencies("B").is_empty());
        assert!(graph.direct_dependencies("C").is_empty());
    }
}
