use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::types::{Graph, Connector, Node};
use crate::utils::log;

type GlobalGraphs = Mutex<HashMap<String, Graph>>;

lazy_static! {
  static ref GRAPHS: GlobalGraphs = Mutex::new(HashMap::new());
}

pub fn cache_set(graph: Graph) {
  let mut graphs = GRAPHS.lock().expect("Could not lock global graph cache.");
  graphs.insert(graph.id.clone(), graph.clone());
}

pub fn get_graph_from_global_store(id: &str) -> Option<Graph> {
  let graphs = GRAPHS.lock().expect("Could not lock global graph store.");
  graphs.get(id).cloned()
}


pub fn parse_graph(json_str: &str) -> Result<Graph, serde_json::Error> {
  serde_json::from_str(json_str)
}

fn integrate_linked_graphs_with_fields(graph: &mut Graph, base_path: &str) {
  let mut global_nodes = Vec::new();
  // Recursively load and integrate linked graphs, now considering the fields
  load_and_integrate_linked_graphs_with_fields(graph, base_path, &mut global_nodes);
  // Update the top-level graph's nodes with all integrated nodes
  graph.nodes = global_nodes;
}

fn load_and_integrate_linked_graphs_with_fields(graph: &mut Graph, base_path: &str, global_nodes: &mut Vec<Node>) {
  for node in graph.nodes.iter_mut() {
      if let Some(linked_graph) = node.linked_graph.take() { // Temporarily take ownership
          // Construct the path to the linked graph
          let path = linked_graph.url;
          let mut loaded_graph = load_graph_from_file(&path);
          // Update node IDs and connectors for inputs and outputs
          for input in linked_graph.fields.inputs.values() {
              let connector = Connector {
                  id: input.id.clone(),
                  node_id: input.id.to_string(),
                  field: input.field.clone(),
                  graph_id: graph.id.clone(),
                  version: 0,
              };
              // Find the corresponding node and add this connector
              if let Some(node) = global_nodes.iter_mut().find(|n| n.id == connector.node_id) {
                  if let Some(edge) = node.edges.iter_mut().find(|e| e.field == connector.field) {
                      edge.connectors.push(connector);
                  }
              }
          }
          load_and_integrate_linked_graphs_with_fields(&mut loaded_graph, base_path, global_nodes);
      }
  }
  global_nodes.append(&mut graph.nodes);
}

pub fn load_graph_from_file(path: &str) -> Graph {
  log("_load_graph_from_file", format!("path: {}", path));
  let graph_string = std::fs::read_to_string(path)
      .expect("Failed to read test data file");
  let mut graph = parse_graph(&graph_string)
      .expect("Error parsing JSON into Graph");
  integrate_linked_graphs_with_fields(&mut graph, "");
  cache_set(graph.clone());
  return graph;
}
