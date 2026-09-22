use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::types::{Graph, Node};
use crate::utils::log;

type GlobalGraphs = Mutex<HashMap<String, Graph>>;

lazy_static! {
  static ref GRAPHS: GlobalGraphs = Mutex::new(HashMap::new());
}

pub fn set_graph_to_global_store(graph: Graph) {
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
  /*
   * The nodes of this level join the flat set *first*, so that rewiring can
   * see them.  The original appended them at the end of the walk, which meant
   * the connectors it was trying to rewire were not in the list yet and the
   * input mapping did nothing at all at the top level.
   */
  let graph_id = graph.id.clone();
  let start = global_nodes.len();
  global_nodes.append(&mut graph.nodes);
  let mut index = start;
  while index < global_nodes.len() {
    let linked_graph = global_nodes[index].linked_graph.take();
    if let Some(linked_graph) = linked_graph {
      let host_id = global_nodes[index].id.clone();
      let host_edges = global_nodes[index].edges.clone();
      let path = linked_graph.url.clone();
      let mut loaded_graph = file(&path);
      /*
       * A connector pointing at *this host node* is pointing at whatever the
       * host's input names.  The inner loop used to call its own item `node`,
       * which shadows the host: the test then asked whether a connector points
       * at the node that owns it — a self-loop, never what was meant.  The
       * field is matched too, so a subgraph with more than one way in is wired
       * the way it was drawn instead of to whichever input came last.
       */
      let single_input = linked_graph.fields.inputs.len() == 1;
      for (host_field, input) in linked_graph.fields.inputs.iter() {
        for other in global_nodes.iter_mut() {
          for edge in other.edges.iter_mut() {
            for connector in edge.connectors.iter_mut() {
              if connector.node_id == host_id && (&connector.field == host_field || single_input) {
                log("linked_graph: input", format!("node: {}, field: {}, graph_id: {}", input.id, input.field, graph_id));
                connector.node_id = input.id.clone();
                connector.field = input.field.clone();
              }
            }
          }
        }
      }
      // What left the host leaves the inner node that produces it.  This used
      // to `return` when an edge was not the one named, abandoning the whole
      // walk and leaving every node after it unintegrated.
      for (host_field, output) in linked_graph.fields.outputs.iter() {
        let edge = match host_edges.iter().find(|edge| &edge.field == host_field) {
          Some(edge) => edge,
          None => continue,
        };
        if let Some(inner_node) = loaded_graph.nodes.iter_mut().find(|n| n.id == output.id) {
          if let Some(inner_edge) = inner_node.edges.iter_mut().find(|e| e.field == output.field) {
            log("linked_graph: output", format!("node: {}, field: {}, graph_id: {}", output.id, output.field, graph_id));
            for connector in edge.connectors.iter() {
              inner_edge.connectors.push(connector.clone());
            }
          }
        }
      }
      load_and_integrate_linked_graphs_with_fields(&mut loaded_graph, base_path, global_nodes);
    }
    index += 1;
  }
}

/*
 * Loading no longer flattens.  A linked graph is instantiated when a value
 * reaches it (see `instances.rs`), which is what lets two uses of one subgraph
 * keep their own state and lets a graph contain itself at all — flattening
 * that would never finish.  `flatten` is kept for anyone who wants the old
 * one-set-of-nodes arrangement, and its two long-standing faults are fixed.
 */
pub fn json(graph_string: &str) -> Graph {
  log("json", format!("graph_string: {}", graph_string));
  let graph = parse_graph(&graph_string)
      .expect("Error parsing JSON into Graph");
  set_graph_to_global_store(graph.clone());
  return graph;
}


pub fn file(path: &str) -> Graph {
  log("load_graph_from_file", format!("path: {}", path));
  let graph_string = std::fs::read_to_string(path)
      .expect("Failed to read test data file");
  let graph = parse_graph(&graph_string)
      .expect("Error parsing JSON into Graph");
  set_graph_to_global_store(graph.clone());
  return graph;
}

/// The old arrangement: every linked graph copied into the host before
/// anything runs.  One set of nodes, shared by every use of a subgraph.
pub fn flatten(graph: &Graph) -> Graph {
  let mut copy = graph.clone();
  integrate_linked_graphs_with_fields(&mut copy, "");
  copy
}

