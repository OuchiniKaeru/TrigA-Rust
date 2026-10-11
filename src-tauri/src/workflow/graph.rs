use super::model::WorkflowDefinition;
use std::collections::{HashMap, HashSet};

/// Graph helpers: adjacency, predecessors, topo order (re-export).
pub struct Graph<'a> {
    pub def: &'a WorkflowDefinition,
    pub outgoing: HashMap<String, Vec<(String, String)>>,
    pub incoming: HashMap<String, usize>,
}

impl<'a> Graph<'a> {
    pub fn build(def: &'a WorkflowDefinition) -> Self {
        let mut outgoing: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut incoming: HashMap<String, usize> = HashMap::new();
        for n in &def.nodes {
            incoming.entry(n.id.clone()).or_insert(0);
        }
        for e in &def.edges {
            outgoing
                .entry(e.source.clone())
                .or_default()
                .push((e.source_port.clone(), e.target.clone()));
            *incoming.entry(e.target.clone()).or_insert(0) += 1;
        }
        Self {
            def,
            outgoing,
            incoming,
        }
    }

    pub fn start_nodes(&self) -> Vec<String> {
        let mut starts: Vec<String> = self
            .incoming
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        starts.sort();
        starts
    }

    /// Edges grouped by source port for a node.
    pub fn next_via(&self, node_id: &str, port: &str) -> Vec<String> {
        self.outgoing
            .get(node_id)
            .map(|v| {
                v.iter()
                    .filter(|(p, _)| p == port)
                    .map(|(_, t)| t.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// All downstream nodes reachable via any port.
    pub fn downstream(&self, node_id: &str) -> Vec<String> {
        self.outgoing
            .get(node_id)
            .map(|v| v.iter().map(|(_, t)| t.clone()).collect())
            .unwrap_or_default()
    }

    pub fn node_ids(&self) -> HashSet<String> {
        self.def.nodes.iter().map(|n| n.id.clone()).collect()
    }
}
