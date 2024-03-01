use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct EventEmitter {
    pub subscribers: Arc<Mutex<HashMap<EventType, Vec<Arc<dyn Fn(Event) + Send + Sync>>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub enum EventType {
    AfterSet,
    Begin,
    BeginConnector,
    BeginEdge,
    End,
    EndConnector,
    EndEdge,
    Error,
    Load,
    Set,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_type: EventType,
    pub data: serde_json::Value, // Use a flexible data type to accommodate different event payloads
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusMessage {
    pub value: serde_json::Value,
    pub scheduler_id: String,
    pub node_id: String,
    pub graph_id: String,
    pub connector_id: String,
    pub field: String,
    pub connector_field: String,
    pub connector_graph_id: String,
    pub connector_node_id: String,
    pub edge_field: String,
    pub caller_graph_id: String,
    pub caller_node_id: String,
    pub caller_scheduler_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub id: String,
    pub url: String,
    pub nodes: Vec<Node>,
    pub properties: GraphProperties,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphProperties {
    pub name: String,
    pub description: String,
    pub created_by: String,
    pub created_on: DateTime<Utc>,
    pub last_update: DateTime<Utc>,
    pub exportable: bool,
    pub height: u32,
    pub width: u32,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub linked_graph: Option<LinkedGraph>,
    pub linked_node: Option<LinkedNode>,
    pub edges: Vec<Edge>,
    pub version: u32,
    pub graph_id: String,
    pub url: String,
    pub data: String,
    pub properties: HashMap<String, Value>,
    pub template: NodeTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub field: String,
    pub connectors: Vec<Connector>,
    pub external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    pub id: String,
    pub node_id: String,
    pub field: String,
    pub graph_id: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldMap {
    pub id: String,
    pub field: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedNode {
    pub id: String,
    pub version: u32,
    pub node: Option<Box<Node>>,
    pub loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTemplate {
    pub set: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedGraph {
    pub id: String,
    pub url: String,
    pub version: u32,
    pub graph: Option<Box<Graph>>,
    pub loaded: bool,
    pub data: HashMap<String, serde_json::Value>,
    pub properties: HashMap<String, serde_json::Value>,
    pub fields: LinkedGraphFields,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedGraphFields {
    pub inputs: HashMap<String, FieldMap>,
    pub outputs: HashMap<String, FieldMap>,
}

#[derive(Clone)]
pub struct Scheduler {
    pub graph: Graph,
    pub id: String,
    pub event_emitter: Arc<EventEmitter>,
}