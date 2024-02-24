use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use rusty_v8 as v8;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};
use std::thread::{self, JoinHandle};
use uuid::Uuid;

static V8_INIT: Once = Once::new();

type GlobalEventEmitters = Arc<Mutex<HashMap<String, Arc<EventEmitter>>>>;
type GlobalGraphs = Mutex<HashMap<String, Graph>>;

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
            let mut handles: Vec<JoinHandle<()>> = Vec::new();

            for callback in callbacks.iter().cloned() {
                // Clone the Arc<dyn Fn(Event) + Send + Sync>
                let event_clone = event.clone();
                // Spawn the thread and store its JoinHandle
                let handle = thread::spawn(move || {
                    callback(event_clone);
                });
                handles.push(handle);
            }

            // Wait for all threads to complete
            for handle in handles {
                handle.join().expect("Thread panicked");
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

pub fn parse_graph(json_str: &str) -> Result<Graph, serde_json::Error> {
    serde_json::from_str(json_str)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerCaller {
    pub scheduler_id: String,
    pub graph_id: String,
    pub node_id: String,
}
pub struct Scheduler {
    pub graph: Graph,
    pub id: String,
    pub event_emitter: Arc<EventEmitter>,
    pub caller: Mutex<SchedulerCaller>,
}

pub fn initialize_v8() {
    V8_INIT.call_once(|| {
        // Initialize V8
        let platform = v8::new_default_platform(8, true).make_shared(); // Make sure to handle Result properly
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

lazy_static! {
    static ref GRAPHS: GlobalGraphs = Mutex::new(HashMap::new());
    static ref EVENT_EMITTERS: GlobalEventEmitters = Arc::new(Mutex::new(HashMap::new()));
    static ref GLOBAL_EVENT_EMITTER: Arc<EventEmitter> = Arc::new(EventEmitter::new());
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
    pub fn new(graph: Graph, scheduler_id: Option<String>) -> Self {
        initialize_v8();

        Scheduler::cache_set(graph.clone());

        let id = scheduler_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut event_emitters = EVENT_EMITTERS.lock().unwrap();
        let emitter = event_emitters.entry(id.clone())
            .or_insert_with(|| Arc::new(EventEmitter::new()))
            .clone();
        let graph_id = graph.id.clone();
        Self {
            event_emitter: emitter,
            graph,
            id,
            caller: Mutex::new(SchedulerCaller {
                graph_id,
                scheduler_id: "".to_string(),
                node_id: "".to_string(),
            }),
        }
    }
    // fn load(id: String) -> Graph {
    //     return get_graph_from_global_store(&id).unwrap();
    // }
    pub fn cache_set(graph: Graph) {
        let mut graphs = GRAPHS.lock().unwrap(); // Acquire the lock
        graphs.insert(graph.id.clone(), graph.clone()); // Insert the graph
    }
    pub fn _load_graph_from_file(path: &str) -> Graph {
        let graph_string = std::fs::read_to_string(path)
            .expect("Failed to read test data file");
        let graph = parse_graph(&graph_string)
            .expect("Error parsing JSON into Graph");
        Scheduler::cache_set(graph.clone());
        return graph;
    }
    fn edge(
        &self,
        scope: &mut v8::ContextScope<'_, v8::HandleScope<'_>>,
        node: Node,
        value: serde_json::Value,
        field: String,
        caller: SchedulerCaller,
    ) {
        // println!("Begin edge traversal scheduler_id: {}", self.id);
        // Define a setter for each edges
        let object_template = v8::ObjectTemplate::new(scope);
        for edge in &node.edges {
            self.event_emitter.emit(Event {
                event_type: EventType::BeginEdge,
                data: serde_json::json!({
                  "graphId": node.graph_id,
                  "nodeId": node.id,
                  "field": field,
                  "edgeField": edge.field,
                  "value": value,
                }),
            });
            let external_connector = Connector {
                id: "external".to_string(),
                node_id: caller.node_id.clone(),
                field: "foo".to_string(),
                graph_id: caller.graph_id.clone(),
                version: 0,
            };
            let has_external_connection = caller.scheduler_id != "" && edge.external;
            let external_connector_vec: Vec<&Connector> = if has_external_connection { vec![&external_connector] } else { Vec::new() };
            let connector_collection = &edge.connectors;
            // figure out what connectors were connected to this URL and send a value to them
            for connector in connector_collection.iter().chain(external_connector_vec) {
                self.event_emitter.emit(Event {
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
                            "schedulerId",
                            "nodeId",
                            "graphId",
                            "connectorId",
                            "connectorField",
                            "connectorGraphId",
                            "connectorNodeId",
                            "connectorVersion",
                            "callerNodeId",
                            "callerGraphId",
                            "callerSchedulerId",
                            "edgeField",
                        ];
                        // Initialize a BusMessage with empty or default values
                        let mut bus_message = BusMessage {
                            value: v8_value_to_serde_json(value, scope),
                            scheduler_id: String::new(),
                            node_id: String::new(),
                            graph_id: String::new(),
                            connector_id: String::new(),
                            field: String::new(),
                            connector_field: String::new(),
                            connector_graph_id: String::new(),
                            connector_node_id: String::new(),
                            edge_field: String::new(),
                            caller_graph_id: String::new(),
                            caller_node_id: String::new(),
                            caller_scheduler_id: String::new(),
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
                                    "schedulerId" => bus_message.scheduler_id = property_str,
                                    "graphId" => bus_message.graph_id = property_str,
                                    "field" => bus_message.field = property_str,
                                    "connectorId" => bus_message.connector_id = property_str,
                                    "connectorField" => bus_message.connector_field = property_str,
                                    "callerGraphId" => bus_message.caller_graph_id = property_str,
                                    "callerNodeId" => bus_message.caller_node_id = property_str,
                                    "callerSchedulerId" => bus_message.caller_scheduler_id = property_str,
                                    "connectorGraphId" => {
                                        bus_message.connector_graph_id = property_str
                                    }
                                    "connectorNodeId" => {
                                        bus_message.connector_node_id = property_str
                                    }
                                    "edgeField" => bus_message.edge_field = property_str,
                                    _ => {} // Handle unexpected properties if necessary
                                }
                            }
                        }
                        let is_external_connector: bool = bus_message.connector_id == "external";
                        let graph_id = if is_external_connector { bus_message.caller_graph_id } else { bus_message.graph_id.clone() };
                        let node_id = if is_external_connector { bus_message.caller_node_id } else { bus_message.connector_node_id.clone() };
                        let graph = get_graph_from_global_store(&graph_id).unwrap();
                        let scheduler = Scheduler::new(graph.clone(), Some(bus_message.scheduler_id.clone()));
                        let target_field = if is_external_connector {graph
                            .clone()
                            .nodes
                            .iter()
                            .find(|node| node.id == node_id)
                            .unwrap().linked_graph
                            .clone()
                            .unwrap()
                            .fields.outputs
                            .get(&bus_message.field.clone())
                            .unwrap()
                            .field
                            .clone()} else {bus_message.field.clone()};
                        let edge_caller = SchedulerCaller {
                            graph_id: bus_message.graph_id,
                            scheduler_id: bus_message.scheduler_id.clone(),
                            node_id: bus_message.connector_node_id,
                        };
                        scheduler.execute_node_by_id(
                            node_id,
                            bus_message.value.clone(),
                            target_field,
                            edge_caller.clone(),
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
                // println!("add to template: scheduler_id: {}", self.id);
                set_key_value(scope, "schedulerId", &self.id.to_string(), object_instance);
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
                set_key_value(scope, "edgeField", &edge.field, object_instance);
                set_key_value(scope, "field", &field, object_instance);
                set_key_value(scope, "callerGraphId", &caller.graph_id, object_instance);
                set_key_value(scope, "callerNodeId", &caller.node_id, object_instance);
                set_key_value(scope, "callerSchedulerId", &caller.scheduler_id, object_instance);
                let value_key = v8::String::new(scope, "value").unwrap();
                let value_value = serde_json_to_v8(scope, &value);
                let global = scope.get_current_context().global(scope);
                let object_key = v8::String::new(scope, "edges").unwrap();
                let try_set_val = global.set(scope, value_key.into(), value_value.into());
                if try_set_val.is_some() {
                    println!("Cannot set value because it is None");
                    try_set_val.unwrap();
                }
                global
                    .set(scope, object_key.into(), object_instance.into())
                    .unwrap();
                self.event_emitter.emit(Event {
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
            self.event_emitter.emit(Event {
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
        self.event_emitter.emit(Event {
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

        // Attempt to compile the script
        let script = v8::Script::compile(try_catch, code, None);

        if script.is_none() && try_catch.has_caught() {
            // Compilation failed with an exception
            let exception_string = try_catch.exception().unwrap().to_rust_string_lossy(try_catch);
            println!("{} compile error {}", self.id, exception_string);
            self.event_emitter.emit(Event {
                event_type: EventType::Error,
                data: serde_json::json!({
                    "graphId": node.graph_id,
                    "nodeId": node.id,
                    "field": field,
                    "value": value,
                    "error": exception_string,
                }),
            });
        } else if let Some(compiled_script) = script {
            // Compilation succeeded, now try to run the script
            let result = compiled_script.run(try_catch);
            match result {
                Some(result_str) => {
                    // Script execution succeeded
                    println!("{} set result return {}", self.id, result_str.to_rust_string_lossy(try_catch));
                    self.event_emitter.emit(Event {
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
                    // Script execution failed with an exception
                    let exception_string = try_catch.exception().unwrap().to_rust_string_lossy(try_catch);
                    println!("{} set result error {}", self.id, exception_string);
                    self.event_emitter.emit(Event {
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
                let root_caller = SchedulerCaller {
                    graph_id: self.graph.id.clone(),
                    scheduler_id: self.id.clone(),
                    node_id: node.id.clone(),
                };

                self.execute_node_by_id(node.id.clone(), value, field, root_caller);

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

    fn execute_node_by_id(&self, id: String, value: serde_json::Value, field: String, caller: SchedulerCaller) {
        // find a node with the given URL
        println!("id {}", id);
        println!("graph {:?}", self.graph);
        let node_option: Option<&Node> = self.graph.nodes.iter().find(|&node| node.id == id);
        match node_option {
            Some(node) => {
                // if this is a linked node, then load the linked graph and invoke the linked graph here
                if node.linked_graph.is_some() {
                    let linked_graph = node.linked_graph.clone().unwrap();
                    let full_linked_graph = get_graph_from_global_store(&linked_graph.id).unwrap();
                    let scheduler = Scheduler::new(full_linked_graph.clone(), None);
                    let proxy_field = linked_graph.fields.inputs.get(&field).unwrap();
                    let entry_node_option:&Node = full_linked_graph.nodes.iter().find(|&node| node.id == proxy_field.id).unwrap();
                    scheduler.url(entry_node_option.url.clone(), value, proxy_field.field.clone());
                    {
                        let mut caller_guard = scheduler.caller.lock().unwrap();
                        *caller_guard = caller;
                    }
                    return;
                }

                // Create and return a new Isolate. Ownership is transferred to the caller.
                let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
                // Directly create a handle scope with the owned isolate.
                let handle_scope = &mut v8::HandleScope::new(isolate);
                let context = v8::Context::new(handle_scope);
                let scope: &mut v8::ContextScope<'_, v8::HandleScope<'_>> =
                    &mut v8::ContextScope::new(handle_scope, context);
                // Process messages after setting up V8 context and running scripts
                println!("Traverse edge for scheduler_id {}", self.id);
                self.edge(scope, node.clone(), value, field, caller);
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

    #[test]
    fn minimal_viable_graph() {
        let graph = Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_minimal.json");
        let scheduler = Scheduler::new(graph, None);
        assert_eq!(scheduler.graph.id, "graph2", "The graph id did not match 'graph1'");
    }

    #[tokio::test]
    async fn single_node_js_invoke() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let graph = Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_with_one_js_test.json");
        let scheduler = Scheduler::new(graph, None);
        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });
        scheduler.url(
            "node1".to_string(),
            serde_json::Value::String("value".to_string()),
            "field".to_string(),
        );
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        assert_eq!(value, "Hello, world!", "Expected to see another value here.");
    }

    #[tokio::test]
    async fn graph_with_edge() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let graph = Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_with_edge.json");
        let scheduler = Scheduler::new(graph, None);
        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });
        scheduler.url(
            "node1".to_string(),
            serde_json::Value::String("value".to_string()),
            "field".to_string(),
        );
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        // println!("{}", value);
        assert_eq!(value, "Hello, world from node2!", "Expected to see another value here.");
    }

    #[tokio::test]
    async fn graph_with_two_edges() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let graph = Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_with_two_edges.json");
        let scheduler = Scheduler::new(graph, None);
        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });
        scheduler.url(
            "node1".to_string(),
            serde_json::Value::String("value".to_string()),
            "field".to_string(),
        );
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        println!("{}", value);
        assert_eq!(value, "End of the line. Data processed in Node3.", "Expected to see another value here.");
    }

    #[tokio::test]
    async fn async_graph_with_two_edges_then_error() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let graph = Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_with_two_edges_then_error.json");
        let scheduler = Scheduler::new(graph, None);

        scheduler.event_emitter.subscribe(EventType::Error, move |event| {
            let _ = tx.try_send(event);
        });

        scheduler.url(
            "node1".to_string(),
            serde_json::Value::String("value".to_string()),
            "field".to_string(),
        );

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed unexpectedly");

        let error = event.data.get("error").and_then(serde_json::Value::as_str)
            .expect("Could not find error key.");
        assert_eq!(error, "TypeError: Cannot read properties of undefined (reading 'cause')", "Did not see expected error message.");
    }

    #[tokio::test]
    async fn async_linked_graph() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_linked.json");
        let graph = Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_proxy_to_log.json");
        let scheduler = Scheduler::new(graph, None);

        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });

        let test_value = "foo";

        scheduler.url(
            "index".to_string(),
            serde_json::Value::String(test_value.to_string()),
            "field".to_string(),
        );

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        println!("{}", value);
        assert_eq!(value, test_value, "Expected to see another value here.");

    }

    #[tokio::test]
    async fn async_linked_cycle_graph() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_cycle_inner.json");
        let graph = Scheduler::_load_graph_from_file("tests/fixtures/graphs/graph_cycle_outer.json");
        let scheduler = Scheduler::new(graph, None);

        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });

        let test_value = "foo";

        scheduler.url(
            "index".to_string(),
            serde_json::Value::String(test_value.to_string()),
            "field".to_string(),
        );

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        println!("{}", value);
        assert_eq!(value, test_value, "Expected to see another value here.");

    }

}
