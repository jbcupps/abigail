use abigail_memory::{
    cosine_similarity, ConversationTurn, EdgeType, Memory, MemoryEdge, MemoryGraph, MemoryStore,
};

#[test]
fn ci_smoke_exercises_in_memory_store_graph_and_vector_helpers() {
    let store = MemoryStore::new_for_ci().unwrap();
    let turn = ConversationTurn::new("ci-session", "user", "hello");
    let a = Memory::ephemeral("alpha".into());
    let b = Memory::distilled("beta".into());

    store.insert_turn(&turn).unwrap();
    store.insert_memory(&a).unwrap();
    store.insert_memory(&b).unwrap();
    assert_eq!(store.total_turn_count().unwrap(), 1);
    assert_eq!(store.count_memories().unwrap(), 2);

    let mut graph = MemoryGraph::new();
    graph.add_edge(MemoryEdge::new(&a.id, &b.id, EdgeType::DerivedFrom));
    assert_eq!(graph.edges_from(&a.id).len(), 1);

    assert!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) > 0.99);
}
