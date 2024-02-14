use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use rusty_v8 as v8;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};
use std::thread;

static V8_INIT: Once = Once::new();

#[derive(Clone)]
pub struct EventEmitter {
    subscribers: Arc<Mutex<HashMap<EventType, Vec<Arc<dyn Fn(Event) + Send + Sync>>>>>,
}

impl EventEmitter {
    pub fn new() -> Self {
        EventEmitter {
            subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe<F>(&self, event_type: EventType, callback: F)
    where
        F: Fn(Event) + 'static + Send + Sync,
    {
        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers
            .entry(event_type)
            .or_insert_with(Vec::new)
            .push(Arc::new(callback));
    }

    pub fn emit(&self, event: Event) {
        let subscribers = self.subscribers.lock().unwrap();
        if let Some(callbacks) = subscribers.get(&event.event_type) {
            for callback in callbacks.iter().cloned() {
                // Clone the Arc<dyn Fn(Event) + Send + Sync>
                let event_clone = event.clone();
                thread::spawn(move || {
                    callback(event_clone);
                });
            }
        }
    }
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
    pub node_id: String,
    pub graph_id: String,
    pub connector_id: String,
    pub field: String,
    pub version: String,
    pub connector_field: String,
    pub connector_graph_id: String,
    pub connector_node_id: String,
    pub edge_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub id: String,
    pub url: String,
    pub nodes: Vec<Node>,
    pub properties: GraphProperties,
    pub version: String,
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
    pub linked_graph: Option<Box<LinkedGraph>>,
    pub linked_node: Option<Box<LinkedNode>>,
    pub edges: Vec<Edge>,
    pub version: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    pub id: String,
    pub node_id: String,
    pub field: String,
    pub graph_id: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldMap {
    pub id: String,
    pub field: String,
    pub data_type: String,
    pub external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedNode {
    pub id: String,
    pub version: String,
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
    pub version: String,
    pub graph: Graph,
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

pub fn parse_graph(json_str: &str) -> Result<Graph, serde_json::Error> {
    serde_json::from_str(json_str)
}

#[derive(Clone)]
pub struct Scheduler {
    pub graph: Graph,
    pub event_emitter: Arc<EventEmitter>,
}

pub fn initialize_v8() {
    V8_INIT.call_once(|| {
        // Initialize V8
        let platform = v8::new_default_platform(8, true).make_shared(); // Make sure to handle Result properly
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
        // Start event bus
        // let (tx, rx) = mpsc::channel::<Event>();
        // event_emitter.subscribe(EventType::NodeUpdated, tx);

        // // Somewhere else in a thread or async task
        // for received_event in rx.iter() {
        //     match received_event.event_type {
        //         EventType::NodeUpdated => {
        //             // Handle the NodeUpdated event
        //         },
        //         _ => {}
        //     }
        // }
    });
}

lazy_static! {
    static ref GRAPHS: Mutex<HashMap<String, Graph>> = Mutex::new(HashMap::new());
    static ref GLOBAL_EVENT_EMITTER: Arc<EventEmitter> = Arc::new(EventEmitter::new());
}

pub fn get_global_event_emitter() -> Arc<EventEmitter> {
    GLOBAL_EVENT_EMITTER.clone()
}

fn get_graph_from_global_store(id: &str) -> Option<Graph> {
    let graphs = GRAPHS.lock().unwrap();
    graphs.get(id).cloned()
}

fn serde_json_to_v8<'a>(
    scope: &mut v8::HandleScope<'a>,
    value: &Value,
) -> v8::Local<'a, v8::Value> {
    match value {
        Value::Null => v8::null(scope).into(),
        Value::Bool(b) => v8::Boolean::new(scope, *b).into(),
        Value::Number(num) => {
            if let Some(n) = num.as_f64() {
                v8::Number::new(scope, n).into()
            } else {
                v8::undefined(scope).into()
            }
        }
        Value::String(s) => v8::String::new(scope, s).unwrap().into(),
        Value::Array(arr) => {
            let array = v8::Array::new(scope, arr.len() as i32);
            for (i, item) in arr.iter().enumerate() {
                let v8_item = serde_json_to_v8(scope, item);
                array.set_index(scope, i as u32, v8_item);
            }
            array.into()
        }
        Value::Object(obj) => {
            let object = v8::Object::new(scope);
            for (k, v) in obj {
                let key = v8::String::new(scope, k).unwrap().into();
                let value = serde_json_to_v8(scope, v);
                object.set(scope, key, value).unwrap();
            }
            object.into()
        }
    }
}

fn v8_value_to_serde_json(
    value: v8::Local<v8::Value>,
    scope: &mut v8::HandleScope,
) -> serde_json::Value {
    // println!("v8_value_to_serde_json {}", value);
    if value.is_string() {
        let value = value.to_rust_string_lossy(scope);
        return serde_json::Value::String(value);
    } else if value.is_number() {
        let num = value.to_number(scope).unwrap().value();
        // JavaScript's Number is always a double-precision floating-point format (f64 in Rust)
        if num.fract() == 0.0 {
            // Check if it can be safely represented as an i64
            if num >= i64::MIN as f64 && num <= i64::MAX as f64 {
                serde_json::Value::Number(serde_json::Number::from(num as i64))
            } else {
                // Outside i64 range, keep as f64
                serde_json::to_value(num).unwrap_or(serde_json::Value::Null)
            }
        } else {
            // For non-integer numbers, represent as f64 directly
            serde_json::to_value(num).unwrap_or(serde_json::Value::Null)
        }
    } else if value.is_boolean() {
        let boolean = value.is_true();
        return serde_json::Value::Bool(boolean);
    } else if value.is_null() {
        return serde_json::Value::Null;
    } else if value.is_undefined() {
        // `undefined` is not directly representable in JSON;
        // you might choose to use null or some other convention
        return serde_json::Value::Null;
    } else {
        // Handle arrays, objects, or other types as needed
        return serde_json::Value::Null; // Placeholder for simplicity
    }
}

impl Scheduler {
    pub fn new(graph: Graph) -> Self {
        initialize_v8();
        let mut graphs = GRAPHS.lock().unwrap(); // Acquire the lock
        graphs.insert(graph.id.clone(), graph.clone()); // Insert the graph
        let event_emitter = get_global_event_emitter();
        Self {
            event_emitter,
            graph,
        }
    }
    fn edge(
        scope: &mut v8::ContextScope<'_, v8::HandleScope<'_>>,
        node: Node,
        value: serde_json::Value,
        field: String,
    ) {
        // Define a setter for each edges
        let event_emitter: Arc<EventEmitter> = get_global_event_emitter();
        let object_template = v8::ObjectTemplate::new(scope);
        for edge in &node.edges {
            event_emitter.emit(Event {
                event_type: EventType::BeginEdge,
                data: serde_json::json!({
                  "graphId": node.graph_id,
                  "nodeId": node.id,
                  "field": field,
                  "edgeField": edge.field,
                  "value": value,
                }),
            });
            // figure out what connectors were connected to this URL and send a value to them
            for connector in &edge.connectors {
                event_emitter.emit(Event {
                    event_type: EventType::BeginConnector,
                    data: serde_json::json!({
                      "graphId": node.graph_id,
                      "nodeId": node.id,
                      "connectorId": connector.id,
                      "connectorField": connector.field,
                      "connectorNodeId": connector.node_id,
                      "connectorVersion": connector.version,
                      "connectorGraphId": connector.graph_id,
                      "field": field,
                      "edgeField": edge.field,
                      "value": value,
                    }),
                });
                let setter =
                    |scope: &mut v8::HandleScope<'_>,
                     _: v8::Local<'_, v8::Name>,
                     value: v8::Local<'_, v8::Value>,
                     args: v8::PropertyCallbackArguments<'_>| {
                        let this = args.this();
                        let property_names = [
                            "nodeId",
                            "graphId",
                            "connectorId",
                            "connectorField",
                            "connectorGraphId",
                            "connectorNodeId",
                            "connectorVersion",
                            "edgeField",
                        ];
                        // Initialize a BusMessage with empty or default values
                        let mut bus_message = BusMessage {
                            value: v8_value_to_serde_json(value, scope),
                            node_id: String::new(),
                            graph_id: String::new(),
                            connector_id: String::new(),
                            field: String::new(),
                            version: String::new(),
                            connector_field: String::new(),
                            connector_graph_id: String::new(),
                            connector_node_id: String::new(),
                            edge_field: String::new(),
                        };
                        // Iterate over property names and fetch their values from the V8 object
                        for &property_name in &property_names {
                            let v8_prop_name =
                                v8::String::new(scope, property_name).unwrap().into();
                            if let Some(property_value) = this
                                .get(scope, v8_prop_name)
                                .and_then(|v| v.to_string(scope))
                            {
                                let property_str = property_value.to_rust_string_lossy(scope);
                                // Match property names to fields in BusMessage and assign values
                                match property_name {
                                    "nodeId" => bus_message.node_id = property_str,
                                    "graphId" => bus_message.graph_id = property_str,
                                    "connectorId" => bus_message.connector_id = property_str,
                                    "connectorField" => bus_message.connector_field = property_str,
                                    "connectorGraphId" => {
                                        bus_message.connector_graph_id = property_str
                                    }
                                    "connectorNodeId" => {
                                        bus_message.connector_node_id = property_str
                                    }
                                    "connectorVersion" => bus_message.version = property_str,
                                    "edgeField" => bus_message.edge_field = property_str,
                                    _ => {} // Handle unexpected properties if necessary
                                }
                            }
                        }
                        let graph = get_graph_from_global_store(&bus_message.graph_id).unwrap();
                        let scheduler = Scheduler::new(graph);
                        scheduler.execute_node_by_id(
                            bus_message.connector_node_id,
                            bus_message.value,
                            bus_message.field,
                        );
                    };
                let getter = |scope: &mut v8::HandleScope<'_>,
                              _: v8::Local<'_, v8::Name>,
                              _: v8::PropertyCallbackArguments<'_>,
                              mut rv: v8::ReturnValue<'_>| {
                    let value = v8::Integer::new(scope, 42);
                    rv.set(value.into());
                };
                let getter_setter_key = v8::String::new(scope, &edge.field).unwrap().into();
                object_template.set_accessor_with_setter(getter_setter_key, getter, setter);

                fn set_key_value(
                    scope: &mut v8::HandleScope<'_>,
                    key: &str,
                    value: &str,
                    object_instance: v8::Local<'_, v8::Object>,
                ) {
                    let key_v8 = v8::String::new(scope, key).expect("Failed to create key string");
                    let value_v8 =
                        v8::String::new(scope, value).expect("Failed to create value string");
                    object_instance
                        .set(scope, key_v8.into(), value_v8.into())
                        .expect("Failed to set property");
                }
                let object_instance: v8::Local<'_, v8::Object> =
                    object_template.new_instance(scope).unwrap();
                set_key_value(scope, "nodeId", &node.id, object_instance);
                set_key_value(scope, "graphId", &node.graph_id, object_instance);
                set_key_value(scope, "connectorId", &connector.id, object_instance);
                set_key_value(scope, "connectorField", &connector.field, object_instance);
                set_key_value(
                    scope,
                    "connectorGraphId",
                    &connector.graph_id,
                    object_instance,
                );
                set_key_value(
                    scope,
                    "connectorNodeId",
                    &connector.node_id,
                    object_instance,
                );
                set_key_value(
                    scope,
                    "connectorVersion",
                    &connector.version,
                    object_instance,
                );
                set_key_value(scope, "edgeField", &edge.field, object_instance);
                set_key_value(scope, "field", &field, object_instance);
                let value_key = v8::String::new(scope, "value").unwrap();
                let value_value = serde_json_to_v8(scope, &value);
                object_instance
                    .set(scope, value_key.into(), value_value.into())
                    .expect("Failed to set property");
                let global = scope.get_current_context().global(scope);
                let object_key = v8::String::new(scope, "edges").unwrap();
                global
                    .set(scope, object_key.into(), object_instance.into())
                    .unwrap();
                event_emitter.emit(Event {
                    event_type: EventType::EndConnector,
                    data: serde_json::json!({
                      "graphId": node.graph_id,
                      "nodeId": node.id,
                      "connectorId": connector.id,
                      "connectorField": connector.field,
                      "connectorNodeId": connector.node_id,
                      "connectorVersion": connector.version,
                      "connectorGraphId": connector.graph_id,
                      "field": field,
                      "edgeField": edge.field,
                      "value": value,
                    }),
                });
            }
            event_emitter.emit(Event {
                event_type: EventType::EndEdge,
                data: serde_json::json!({
                  "graphId": node.graph_id,
                  "nodeId": node.id,
                  "field": field,
                  "edgeField": edge.field,
                  "value": value,
                }),
            });
        }
        event_emitter.emit(Event {
            event_type: EventType::Set,
            data: serde_json::json!({
              "graphId": node.graph_id,
              "nodeId": node.id,
              "field": field,
              "value": value,
            }),
        });
        let try_catch = &mut v8::TryCatch::new(scope);
        let code = v8::String::new(try_catch, &node.template.set).unwrap();
        let script = v8::Script::compile(try_catch, code, None).unwrap();

        let result = script.run(try_catch);

        match result {
            Some(result_str) => {
                // The script executed successfully, and `value` is the result.
                event_emitter.emit(Event {
                    event_type: EventType::AfterSet,
                    data: serde_json::json!({
                      "graphId": node.graph_id,
                      "nodeId": node.id,
                      "field": field,
                      "value": value,
                      "return": result_str.to_rust_string_lossy(try_catch),
                    }),
                });
            },
            None => {
                let exception = try_catch.exception().unwrap();
                let exception_string = exception.to_rust_string_lossy(try_catch);
                event_emitter.emit(Event {
                    event_type: EventType::Error,
                    data: serde_json::json!({
                      "graphId": node.graph_id,
                      "nodeId": node.id,
                      "field": field,
                      "value": value,
                      "error": exception_string,
                    }),
                });
            },
        }
    }
    pub fn url(&self, url: String, value: serde_json::Value, field: String) {
        let node_options: Option<&Node> = self.graph.nodes.iter().find(|&node| node.url == url);
        match node_options {
            Some(node) => {
                self.event_emitter.emit(Event {
                    event_type: EventType::Begin,
                    data: serde_json::json!({
                      "field": field,
                      "value": value,
                      "url": url,
                      "nodeId": node.id,
                      "graphId": node.graph_id,
                    }),
                });
                self.execute_node_by_id(node.id.clone(), value, field);
                self.event_emitter.emit(Event {
                    event_type: EventType::End,
                    data: serde_json::json!({
                      "url": url,
                      "nodeId": node.id,
                      "graphId": node.graph_id,
                    }),
                });
            }
            None => {
                eprintln!("Cannot find node URL {}", url);
                std::process::exit(1);
            }
        }
    }

    fn execute_node_by_id(&self, id: String, value: serde_json::Value, field: String) {
        // find a node with the given URL
        let node_options: Option<&Node> = self.graph.nodes.iter().find(|&node| node.id == id);
        match node_options {
            Some(node) => {
                // Create and return a new Isolate. Ownership is transferred to the caller.
                let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
                // Directly create a handle scope with the owned isolate.
                let handle_scope = &mut v8::HandleScope::new(isolate);
                let context = v8::Context::new(handle_scope);
                let scope: &mut v8::ContextScope<'_, v8::HandleScope<'_>> =
                    &mut v8::ContextScope::new(handle_scope, context);
                // Process messages after setting up V8 context and running scripts
                Scheduler::edge(scope, node.clone(), value, field);
            }
            None => {
                eprintln!("Cannot find node ID {}", id);
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn minimal_viable_graph() {
        let minimal_graph_path = Path::new("tests/fixtures/graphs/minimal_graph.json");
        let minimal_graph_string =
            fs::read_to_string(minimal_graph_path).expect("Failed to read test data file");
        match parse_graph(&minimal_graph_string) {
            Ok(graph) => {
                Scheduler::new(graph);
            }
            Err(e) => {
                eprintln!("Error parsing JSON into Graph: {}", e);
                std::process::exit(1);
            }
        }
    }
    #[test]
    fn single_node_js_invoke() {
        let minimal_graph_path = Path::new("tests/fixtures/graphs/graph_with_one_js_test.json");
        let minimal_graph_string =
            fs::read_to_string(minimal_graph_path).expect("Failed to read test data file");
        match parse_graph(&minimal_graph_string) {
            Ok(graph) => {
                let scheduler = Scheduler::new(graph);
                scheduler.url(
                    "node1".to_string(),
                    serde_json::Value::String("value".to_string()),
                    "field".to_string(),
                );
            }
            Err(e) => {
                eprintln!("Error parsing JSON into Graph: {}", e);
                std::process::exit(1);
            }
        }
    }
    #[test]
    fn graph_with_edge() {
        let minimal_graph_path = Path::new("tests/fixtures/graphs/graph_with_edge.json");
        let minimal_graph_string =
            fs::read_to_string(minimal_graph_path).expect("Failed to read test data file");
        match parse_graph(&minimal_graph_string) {
            Ok(graph) => {
                let scheduler = Scheduler::new(graph);
                scheduler.url(
                    "node1".to_string(),
                    serde_json::Value::String("value".to_string()),
                    "field".to_string(),
                );
            }
            Err(e) => {
                eprintln!("Error parsing JSON into Graph: {}", e);
                std::process::exit(1);
            }
        }
    }
    #[test]
    fn graph_with_two_edges() {
        let path = Path::new("tests/fixtures/graphs/graph_with_two_edges.json");
        let graph_string = fs::read_to_string(path).expect("Failed to read test data file");
        match parse_graph(&graph_string) {
            Ok(graph) => {
                let scheduler = Scheduler::new(graph);
                scheduler
                    .event_emitter
                    .subscribe(EventType::Begin, |event| {
                        println!("Node Begin: {:?}", event);
                    });
                scheduler.url(
                    "node1".to_string(),
                    serde_json::Value::String("value".to_string()),
                    "field".to_string(),
                );
            }
            Err(e) => {
                eprintln!("Error parsing JSON into Graph: {}", e);
                std::process::exit(1);
            }
        }
    }
    #[test]
    fn graph_with_two_edges_then_error() {
        let path = Path::new("tests/fixtures/graphs/graph_with_two_edges_then_error.json");
        let graph_string = fs::read_to_string(path).expect("Failed to read test data file");
        match parse_graph(&graph_string) {
            Ok(graph) => {
                let scheduler = Scheduler::new(graph);
                scheduler
                    .event_emitter
                    .subscribe(EventType::Error, |event| {
                        println!("Node Error: {:?}", event);
                    });
                scheduler.url(
                    "node1".to_string(),
                    serde_json::Value::String("value".to_string()),
                    "field".to_string(),
                );
            }
            Err(e) => {
                eprintln!("Error parsing JSON into Graph: {}", e);
                std::process::exit(1);
            }
        }
    }
}
