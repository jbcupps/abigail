//! Memory graph edges — relationships between memories.
//!
//! Supports DerivedFrom, CritiquedBy, and RefinedTo edge types.

use serde::{Deserialize, Serialize};

/// Types of edges between memories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    /// Memory B was derived from Memory A.
    DerivedFrom,
    /// Memory B critiques Memory A.
    CritiquedBy,
    /// Memory B is a refinement of Memory A.
    RefinedTo,
}

impl EdgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeType::DerivedFrom => "derived_from",
            EdgeType::CritiquedBy => "critiqued_by",
            EdgeType::RefinedTo => "refined_to",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "derived_from" => Some(EdgeType::DerivedFrom),
            "critiqued_by" => Some(EdgeType::CritiquedBy),
            "refined_to" => Some(EdgeType::RefinedTo),
            _ => None,
        }
    }
}

/// An edge in the memory graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEdge {
    pub id: String,
    pub from_id: String,
    pub to_id: String,
    pub edge_type: EdgeType,
    pub weight: f32,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
}

impl MemoryEdge {
    pub fn new(from_id: &str, to_id: &str, edge_type: EdgeType) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from_id: from_id.to_string(),
            to_id: to_id.to_string(),
            edge_type,
            weight: 1.0,
            metadata: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// In-memory graph store (used with SQLite backend).
/// For Postgres, edges are stored directly in the database.
#[derive(Debug, Default)]
pub struct MemoryGraph {
    edges: Vec<MemoryEdge>,
}

impl MemoryGraph {
    pub fn new() -> Self {
        Self { edges: Vec::new() }
    }

    /// Add an edge to the graph.
    pub fn add_edge(&mut self, edge: MemoryEdge) {
        self.edges.push(edge);
    }

    /// Get all edges from a memory.
    pub fn edges_from(&self, memory_id: &str) -> Vec<&MemoryEdge> {
        self.edges
            .iter()
            .filter(|e| e.from_id == memory_id)
            .collect()
    }

    /// Get all edges to a memory.
    pub fn edges_to(&self, memory_id: &str) -> Vec<&MemoryEdge> {
        self.edges.iter().filter(|e| e.to_id == memory_id).collect()
    }

    /// Get all edges of a specific type from a memory.
    pub fn edges_of_type(&self, memory_id: &str, edge_type: &EdgeType) -> Vec<&MemoryEdge> {
        self.edges
            .iter()
            .filter(|e| e.from_id == memory_id && &e.edge_type == edge_type)
            .collect()
    }

    /// Traverse the graph from a starting node, following edges up to a depth.
    pub fn traverse(&self, start_id: &str, max_depth: u32) -> Vec<String> {
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let mut result = Vec::new();

        queue.push_back((start_id.to_string(), 0u32));
        visited.insert(start_id.to_string());

        while let Some((current, depth)) = queue.pop_front() {
            result.push(current.clone());

            if depth >= max_depth {
                continue;
            }

            for edge in self.edges_from(&current) {
                if !visited.contains(&edge.to_id) {
                    visited.insert(edge.to_id.clone());
                    queue.push_back((edge.to_id.clone(), depth + 1));
                }
            }
        }

        result
    }

    /// Remove all edges involving a memory.
    pub fn remove_edges_for(&mut self, memory_id: &str) {
        self.edges
            .retain(|e| e.from_id != memory_id && e.to_id != memory_id);
    }

    /// Total edge count.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_types() {
        assert_eq!(EdgeType::DerivedFrom.as_str(), "derived_from");
        assert_eq!(EdgeType::parse("critiqued_by"), Some(EdgeType::CritiquedBy));
        assert_eq!(EdgeType::parse("unknown"), None);
    }

    #[test]
    fn test_memory_edge_creation() {
        let edge = MemoryEdge::new("a", "b", EdgeType::DerivedFrom).with_weight(0.8);
        assert_eq!(edge.from_id, "a");
        assert_eq!(edge.to_id, "b");
        assert_eq!(edge.weight, 0.8);
    }

    #[test]
    fn test_graph_add_and_query() {
        let mut graph = MemoryGraph::new();
        graph.add_edge(MemoryEdge::new("a", "b", EdgeType::DerivedFrom));
        graph.add_edge(MemoryEdge::new("a", "c", EdgeType::RefinedTo));
        graph.add_edge(MemoryEdge::new("b", "d", EdgeType::CritiquedBy));

        assert_eq!(graph.edge_count(), 3);
        assert_eq!(graph.edges_from("a").len(), 2);
        assert_eq!(graph.edges_to("b").len(), 1);
        assert_eq!(graph.edges_of_type("a", &EdgeType::DerivedFrom).len(), 1);
    }

    #[test]
    fn test_graph_traverse() {
        let mut graph = MemoryGraph::new();
        graph.add_edge(MemoryEdge::new("a", "b", EdgeType::DerivedFrom));
        graph.add_edge(MemoryEdge::new("b", "c", EdgeType::DerivedFrom));
        graph.add_edge(MemoryEdge::new("c", "d", EdgeType::DerivedFrom));

        let result = graph.traverse("a", 2);
        assert!(result.contains(&"a".to_string()));
        assert!(result.contains(&"b".to_string()));
        assert!(result.contains(&"c".to_string()));
        // "d" is at depth 3, so it should not be included
        assert!(!result.contains(&"d".to_string()));
    }

    #[test]
    fn test_graph_traverse_depth_0() {
        let mut graph = MemoryGraph::new();
        graph.add_edge(MemoryEdge::new("a", "b", EdgeType::DerivedFrom));

        let result = graph.traverse("a", 0);
        assert_eq!(result, vec!["a".to_string()]);
    }

    #[test]
    fn test_remove_edges() {
        let mut graph = MemoryGraph::new();
        graph.add_edge(MemoryEdge::new("a", "b", EdgeType::DerivedFrom));
        graph.add_edge(MemoryEdge::new("b", "c", EdgeType::DerivedFrom));

        graph.remove_edges_for("b");
        assert_eq!(graph.edge_count(), 0);
    }
}
