use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::types::{Graph, Node, LinkedGraph};
use crate::utils::log;

/// Linked graphs as calls, not as copies made in advance.
///
/// The loader used to flatten every linked graph into the host before anything
/// ran: one set of nodes, shared by every use of that subgraph, and no way at
/// all to run a graph that contains itself — flattening it would never finish.
///
/// A linked graph is instantiated here instead, **when a value arrives at it**.
/// The instance is named by the path of host nodes it was reached through,
/// which gives three things at once:
///
///   * the same host reached twice is the same instance, so its nodes and its
///     state carry over between calls;
///   * two hosts carrying the same graph are two instances that share nothing;
///   * a graph reached through itself is a *deeper path*, so it is a new
///     instance with its own everything.  That is recursion, and the path
///     length is the depth.
///
/// When a recursion stops is the graph's business, as it is for any recursive
/// function.  `DEPTH_LIMIT` is the ceiling for one that does not, and it fails
/// naming the path it took.

/// One live copy of a linked graph.
#[derive(Debug, Clone)]
pub struct Instance {
    /// The host nodes this instance was reached through, outermost first.
    pub path: Vec<String>,
    /// How many linked graphs deep this is; the root graph is 0.
    pub depth: usize,
    /// This instance's own copy of the document.
    pub graph: Graph,
    /// The instance that called this one, if it was not the root graph.
    pub parent_key: Option<String>,
    /// The graph this instance was reached from, so what leaves it can get back.
    pub parent_graph_id: String,
    /// State belonging to this instance alone.
    pub state: serde_json::Value,
}

impl Instance {
    pub fn key(&self) -> String {
        self.path.join("/")
    }
}

type Instances = Mutex<HashMap<String, Instance>>;
type Documents = Mutex<HashMap<String, Graph>>;

lazy_static! {
    static ref INSTANCES: Instances = Mutex::new(HashMap::new());
    /// Documents as they were before anything ran, by where they came from.
    static ref DOCUMENTS: Documents = Mutex::new(HashMap::new());
}

/// How deep linked graphs may go before the scheduler calls it a runaway.
pub const DEPTH_LIMIT: usize = 32;

pub fn get_instance(key: &str) -> Option<Instance> {
    let instances = INSTANCES.lock().expect("Could not lock the instance store.");
    instances.get(key).cloned()
}

/// What this instance has to say for itself next time a node in it runs.
pub fn set_instance_state(key: &str, state: serde_json::Value) {
    let mut instances = INSTANCES.lock().expect("Could not lock the instance store.");
    if let Some(instance) = instances.get_mut(key) {
        instance.state = state;
    }
}

/// Between runs, so one test cannot see another's calls.
pub fn reset_instances() {
    INSTANCES.lock().expect("Could not lock the instance store.").clear();
    DOCUMENTS.lock().expect("Could not lock the document store.").clear();
}

pub fn instance_count() -> usize {
    INSTANCES.lock().expect("Could not lock the instance store.").len()
}

pub fn instance_keys() -> Vec<String> {
    let instances = INSTANCES.lock().expect("Could not lock the instance store.");
    let mut keys: Vec<String> = instances.keys().cloned().collect();
    keys.sort();
    keys
}

/// The document as it was before anything ran; instances are made from this.
fn document_for(linked: &LinkedGraph, load: &dyn Fn(&str) -> Option<Graph>) -> Option<Graph> {
    if let Some(graph) = &linked.graph {
        return Some((**graph).clone());
    }
    let source = if linked.url.is_empty() { linked.id.clone() } else { linked.url.clone() };
    {
        let documents = DOCUMENTS.lock().expect("Could not lock the document store.");
        if let Some(document) = documents.get(&source) {
            return Some(document.clone());
        }
    }
    // Loaded at the moment of the call, not in advance: a graph only reached
    // sometimes is only loaded then.
    let loaded = load(&source)?;
    let mut documents = DOCUMENTS.lock().expect("Could not lock the document store.");
    documents.insert(source, loaded.clone());
    Some(loaded)
}

/// The instance a value arriving at this host node belongs to, made if this is
/// the first time that call has been made.
pub fn instance_for(
    host: &Node,
    parent_key: Option<&str>,
    parent_graph_id: &str,
    load: &dyn Fn(&str) -> Option<Graph>,
    depth_limit: usize,
) -> Result<Instance, String> {
    let linked: &LinkedGraph = host.linked_graph.as_ref().ok_or_else(|| {
        format!("Node {} carries no linked graph", host.id)
    })?;
    let mut path: Vec<String> = match parent_key {
        Some(key) if !key.is_empty() => key.split('/').map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    };
    path.push(host.id.clone());
    let key = path.join("/");
    if let Some(existing) = get_instance(&key) {
        return Ok(existing);
    }
    if path.len() > depth_limit {
        return Err(format!(
            "Linked graphs went {} deep (limit {}): {}. A graph that contains itself has to stop itself; this is the ceiling, not the plan.",
            path.len(), depth_limit, path.join(" -> ")
        ));
    }
    let document = document_for(linked, load)
        .ok_or_else(|| format!("Critical Error: Linked graph not found on node.id: {}", host.id))?;
    let mut graph = document;
    seed_from_host(&mut graph, linked);
    wire_outputs(&mut graph, host, linked);
    let instance = Instance {
        path: path.clone(),
        depth: path.len(),
        graph,
        parent_key: parent_key.filter(|k| !k.is_empty()).map(|k| k.to_string()),
        parent_graph_id: parent_graph_id.to_string(),
        state: serde_json::json!({}),
    };
    log("instance", format!("made {} at depth {}", key, path.len()));
    let mut instances = INSTANCES.lock().expect("Could not lock the instance store.");
    instances.insert(key, instance.clone());
    Ok(instance)
}

/// What the host says this use of the subgraph starts with.
fn seed_from_host(graph: &mut Graph, linked: &LinkedGraph) {
    for node in graph.nodes.iter_mut() {
        if let Some(data) = linked.data.get(&node.id) {
            node.data = match data {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
        }
        if let Some(serde_json::Value::Object(properties)) = linked.properties.get(&node.id) {
            node.properties = properties.clone().into_iter().collect();
        }
    }
}

/// What leaves this instance leaves by the host's connectors.  They are copied
/// in, not moved: the host belongs to the graph that called this one, and that
/// graph must come out of a run the way it went in.
fn wire_outputs(graph: &mut Graph, host: &Node, linked: &LinkedGraph) {
    for (host_field, output) in linked.fields.outputs.iter() {
        let connectors = match host.edges.iter().find(|edge| &edge.field == host_field) {
            Some(edge) if !edge.connectors.is_empty() => edge.connectors.clone(),
            _ => continue,
        };
        let inner = match graph.nodes.iter_mut().find(|node| node.id == output.id) {
            Some(node) => node,
            None => continue,
        };
        if let Some(edge) = inner.edges.iter_mut().find(|edge| edge.field == output.field) {
            for connector in connectors {
                if !edge.connectors.iter().any(|c| c.id == connector.id) {
                    edge.connectors.push(connector);
                }
            }
        } else {
            inner.edges.push(crate::types::Edge {
                field: output.field.clone(),
                connectors,
                external: true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::file;

    /// A look at what an instance actually holds, for when something does not arrive.
    #[test]
    fn an_instance_carries_the_hosts_connectors_out() {
        reset_instances();
        let outer = file("tests/fixtures/graphs/graph_cycle_outer.json");
        let host = outer.nodes.iter().find(|n| n.linked_graph.is_some()).expect("no host node");
        let load = |source: &str| -> Option<Graph> { Some(file(source)) };
        let instance = instance_for(host, None, &outer.id, &load, DEPTH_LIMIT).expect("could not instantiate");
        let producer = instance.graph.nodes.iter()
            .find(|n| n.id == "e4431ec2-3e67-4a84-a9cf-76fbc434ddaa")
            .expect("no producing node in the instance");
        let edge = producer.edges.iter().find(|e| e.field == "indigo")
            .unwrap_or_else(|| panic!("no indigo edge; edges are {:?}", producer.edges.iter().map(|e| e.field.clone()).collect::<Vec<_>>()));
        assert_eq!(edge.connectors.len(), 1, "the host's connector should leave from here");
        assert_eq!(edge.connectors[0].node_id, "5b07b419-d62b-4a2d-9974-fad1666ef9db");
    }
}
