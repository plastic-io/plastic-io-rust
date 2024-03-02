use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::types::{Graph, Node};
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
      // locate the nodes connecting to the host nodes inputs
      // and change those connectors so they are connecting to
      // the inner graph's inputs
      for input in linked_graph.fields.inputs.values() {
        for node in global_nodes.iter_mut() {
          for edge in node.edges.iter_mut() {
            for connector in edge.connectors.iter_mut() {
              if connector.node_id == node.id {
                log("linked_graph: input", format!("output: {}, field: {}, graph_id: {}", input.id, input.field, graph.id));
                connector.node_id = input.id.clone();
                connector.field = input.field.clone();
              }
            }
          }
        }
      }
      // locate the inner node the linked_graph is talking about and move
      // connectors to the linked node from the host node.
      for (output_host_field, output) in linked_graph.fields.outputs {
        for edge in node.edges.iter_mut() {
          if output_host_field != edge.field { return }
          for connector in edge.connectors.iter_mut() {
            // Find the corresponding node and add this connector
            if let Some(inner_node) = loaded_graph.nodes.iter_mut().find(|n| n.id == output.id) {
                if let Some(inner_edge) = inner_node.edges.iter_mut().find(|e| e.field == output.field) {
                    log("linked_graph: output", format!("output: {}, field: {}, graph_id: {}", output.id, output.field, graph.id));
                    // move connector to linked node
                    inner_edge.connectors.push(connector.to_owned());
                }
            }
          }
        }
      }
      load_and_integrate_linked_graphs_with_fields(&mut loaded_graph, base_path, global_nodes);
    }
  }
  global_nodes.append(&mut graph.nodes);
}

pub fn load_graph_from_file(path: &str) -> Graph {
  log("load_graph_from_file", format!("path: {}", path));
  let graph_string = std::fs::read_to_string(path)
      .expect("Failed to read test data file");
  let mut graph = parse_graph(&graph_string)
      .expect("Error parsing JSON into Graph");
  integrate_linked_graphs_with_fields(&mut graph, "");
  cache_set(graph.clone());
  return graph;
}

