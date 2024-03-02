use lazy_static::lazy_static;
use rusty_v8 as v8;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};
use std::thread::{self, JoinHandle};
use uuid::Uuid;
use crate::utils::{serde_json_to_v8, v8_value_to_serde_json, log, increment_sequence_counter};
use crate::loader::{get_graph_from_global_store, cache_set};
use crate::types::*;

static V8_INIT: Once = Once::new();

type GlobalEventEmitters = Arc<Mutex<HashMap<String, Arc<EventEmitter>>>>;

lazy_static! {
    static ref EVENT_EMITTERS: GlobalEventEmitters = Arc::new(Mutex::new(HashMap::new()));

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
        let mut subscribers = self.subscribers.lock().expect("Could not lock subscribers hashmap");
        subscribers
            .entry(event_type)
            .or_insert_with(Vec::new)
            .push(Arc::new(callback));
    }

    pub fn emit(&self, event: Event) {
        let subscribers = self.subscribers.lock().expect("Could not lock subscribers hashmap");
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

impl Scheduler {
    pub fn new(graph: Graph, scheduler_id: Option<String>) -> Self {
        Scheduler::initialize_v8();

        cache_set(graph.clone());

        let id = scheduler_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut event_emitters = EVENT_EMITTERS.lock().expect("Could not lock global event emitter hash map");
        let emitter = event_emitters.entry(id.clone())
            .or_insert_with(|| Arc::new(EventEmitter::new()))
            .clone();

        Self {
            event_emitter: emitter,
            graph,
            id
        }
    }

    pub fn initialize_v8() {
        V8_INIT.call_once(|| {
            // Initialize V8
            let platform = v8::new_default_platform(8, true).make_shared(); // Make sure to handle Result properly
            v8::V8::initialize_platform(platform);
            v8::V8::initialize();
        });
    }

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
    fn edge(
        &self,
        node: Node,
        value: serde_json::Value,
        field: String
    ) {
        // Process messages after setting up V8 context and running scripts
        increment_sequence_counter();
        log("edge", format!("graph_id: {}, node_id: {}, scheduler_id: {}, field: {}, value: {:?}", self.graph.id, node.id, self.id, field, value));
        // Create and return a new Isolate. Ownership is transferred to the caller.
        let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
        // Directly create a handle scope with the owned isolate.
        let handle_scope = &mut v8::HandleScope::new(isolate);
        let context = v8::Context::new(handle_scope);
        let scope: &mut v8::ContextScope<'_, v8::HandleScope<'_>> = &mut v8::ContextScope::new(handle_scope, context);
        // Define a setter for each edges
        let object_template = v8::ObjectTemplate::new(scope);
        let edges_key = v8::String::new(scope, "edges").expect("Could not create v8 edges key");
        let mut connector_count = 0;
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
            for connector in &edge.connectors {
                connector_count += 1;
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
                                v8::String::new(scope, property_name).expect("Failed to set edge properties").into();
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
                        // a new scheduler is created and related to the previous scheduler
                        // by getting events from the global events store hashmap by id
                        // this allows us to keep things alive on the outside as users will
                        // always be connected to the events, not the new schedulers that are created.
                        // this probably has some memory impact that I will optimize later.
                        let graph_id = bus_message.graph_id.clone();
                        let graph = get_graph_from_global_store(&graph_id).expect(&format!("Could not load next graph. graph_id: {}", graph_id));
                        let scheduler = Scheduler::new(graph.clone(), Some(bus_message.scheduler_id.clone()));
                        scheduler.traverse_edge(bus_message);
                    };
                let getter = |scope: &mut v8::HandleScope<'_>,
                              _: v8::Local<'_, v8::Name>,
                              _: v8::PropertyCallbackArguments<'_>,
                              mut rv: v8::ReturnValue<'_>| {
                    let value = v8::Integer::new(scope, 42);
                    rv.set(value.into());
                };
                let getter_setter_key = v8::String::new(scope, &edge.field).expect("Could not create getter/setter key").into();
                object_template.set_accessor_with_setter(getter_setter_key, getter, setter);

                let edges_object: v8::Local<'_, v8::Object> = object_template.new_instance(scope).expect("Could not create v8 object template");
                let global = scope.get_current_context().global(scope);

                let mut objects = Vec::new();
                objects.push(edges_object);
                objects.push(global);
                for obj in objects {
                    // all of these values must be passed to the edge setter object for their use in the "this" object in the setter
                    // this is key to transitioning the v8 setter boundary as you cannot refer to object instances in the setter
                    // these values are also added to global for convenience
                    Scheduler::set_key_value(scope, "schedulerId", &self.id.to_string(), obj);
                    Scheduler::set_key_value(scope, "nodeId", &node.id, obj);
                    Scheduler::set_key_value(scope, "graphId", &self.graph.id, obj);
                    Scheduler::set_key_value(scope, "field", &field, obj);

                    Scheduler::set_key_value(scope, "connectorId", &connector.id, obj);
                    Scheduler::set_key_value(scope, "connectorField", &connector.field, obj);
                    Scheduler::set_key_value(scope, "connectorGraphId", &connector.graph_id, obj);
                    Scheduler::set_key_value(scope, "connectorNodeId", &connector.node_id, obj);
                    Scheduler::set_key_value(scope, "edgeField", &edge.field, obj);
                }

                global.set(scope, edges_key.into(), edges_object.into()).expect("Could not set edges object into global scope");

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

        let global = scope.get_current_context().global(scope);

        Scheduler::set_key_value(scope, "schedulerId", &self.id.to_string(), global);
        Scheduler::set_key_value(scope, "nodeId", &node.id, global);
        Scheduler::set_key_value(scope, "graphId", &node.graph_id, global);
        Scheduler::set_key_value(scope, "field", &field, global);

        // when there are no edges, supply an empty object called 'edges' to the interface
        if connector_count == 0 {
            let edges_object: v8::Local<'_, v8::Object> = object_template.new_instance(scope).expect("Could not create v8 object template");
            global.set(scope, edges_key.into(), edges_object.into()).expect("Could not set edges object into global scope");
        }

        let value_key = v8::String::new(scope, "value").expect("Could not create value key");
        let value_value = serde_json_to_v8(scope, &value);
        let try_set_val = global.set(scope, value_key.into(), value_value.into());
        if try_set_val.is_some() {
            try_set_val.expect("Could not set value into global scope");
        } else {
            log("error", format!("Cannot set value because it is None"));
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
        log("JS run", format!("graph_id: {}, node_id: {}, scheduler_id: {}, code: {}", self.graph.id, node.id, self.id, node.template.set));
        let code = v8::String::new(try_catch, &node.template.set).expect(&format!("Could not set JS code.  graph_id: {}, node_id: {}, scheduler_id: {}, code: {}", self.graph.id, node.id, self.id, node.template.set));

        // Attempt to compile the script
        let script = v8::Script::compile(try_catch, code, None);

        if script.is_none() && try_catch.has_caught() {
            // Compilation failed with an exception
            let exception_string = try_catch.exception().expect("Cannot extract v8 JS compilation exception reason").to_rust_string_lossy(try_catch);
            log("Compile error", format!("graph_id: {}, node_id: {}, scheduler_id: {}, exception_string: {}", self.graph.id, node.id, self.id, exception_string));
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
                    log("JS return", format!("graph_id: {}, node_id: {}, scheduler_id: {}, return: {}", self.graph.id, node.id, self.id, result_str.to_rust_string_lossy(try_catch)));
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
                    let exception_string = try_catch.exception().expect("Cannot extract v8 JS runtime exception reason").to_rust_string_lossy(try_catch);
                    log("JS error", format!("graph_id: {}, node_id: {}, scheduler_id: {}, error: {}, error: {}", self.graph.id, node.id, self.id, exception_string, node.template.set));
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
    pub fn traverse_edge(&self, bus_message: BusMessage) {
        let node_id = bus_message.connector_node_id.clone();
        let target_field = bus_message.connector_field.clone();
        log("edge connector invoke", format!("graph_id: {}, from_node_id: {}, to_node_id: {}, from_field: {}, to_field: {}, value: {:?}, scheduler_id: {}", bus_message.graph_id, bus_message.node_id, node_id, bus_message.edge_field, target_field, bus_message.value.clone(), bus_message.scheduler_id));
        self.execute_node_by_id(
            node_id,
            bus_message.value.clone(),
            target_field
        );
    }
    pub fn url(&self, url: String, value: serde_json::Value, field: String) {
        let node_options: Option<&Node> = self.graph.nodes.iter().find(|&node| node.url == url);
        match node_options {
            Some(node) => {
                log("url", format!("url: {}, graph_id: {}, node_id: {}, scheduler_id: {}, field: {}, value: {:?}", url, self.graph.id, node.id, self.id, field, value));
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
                log("edge: node not found", format!(".  url: {}, graph_id: {}, scheduler_id: {}, field: {}, value: {:?}", url, self.graph.id, self.id, field, value));
                eprintln!("Cannot find node URL {}", url);
                std::process::exit(1);
            }
        }
    }

    fn execute_node_by_id(&self, id: String, value: serde_json::Value, field: String) {
        // an entry point node shows up as having no caller node
        let node_option: Option<&Node> = self.graph.nodes.iter().find(|&node| node.id == id);
        match node_option {
            Some(node) => {
                self.edge(node.clone(), value, field);
            }
            None => {
                log("execute_node_by_id node not found", format!("id: {}, graph_id: {}, scheduler_id: {}, field: {}, value: {:?}", id, self.graph.id, self.id, field, value));
                eprintln!("Cannot find node ID {}", id);
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::loader::load_graph_from_file;
    use super::*;

    #[test]
    fn minimal_viable_graph() {
        let graph = load_graph_from_file("tests/fixtures/graphs/graph_minimal.json");
        let scheduler = Scheduler::new(graph, None);
        assert_eq!(scheduler.graph.url, "foo", "The graph url did not match 'foo'");
    }

    #[tokio::test]
    async fn single_node_js_invoke() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let graph = load_graph_from_file("tests/fixtures/graphs/graph_with_one_js_test.json");
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
        let graph = load_graph_from_file("tests/fixtures/graphs/graph_with_edge.json");
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
        assert_eq!(value, "Hello, world from node2!", "Expected to see another value here.");
    }

    #[tokio::test]
    async fn graph_with_two_edges() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let graph = load_graph_from_file("tests/fixtures/graphs/graph_with_two_edges.json");
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

        let graph = load_graph_from_file("tests/fixtures/graphs/graph_with_two_edges_then_error.json");
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

        let graph = load_graph_from_file("tests/fixtures/graphs/graph_linked.json");
        let scheduler = Scheduler::new(graph, None);

        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });

        let test_value = "foo";
        let test_return_value = "foo bar";

        scheduler.url(
            "index".to_string(),
            serde_json::Value::String(test_value.to_string()),
            "field".to_string(),
        );

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        println!("{}", value);
        assert_eq!(value, test_return_value, "Expected to see another value here.");

    }

    #[tokio::test]
    async fn async_linked_cycle_graph() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let graph = load_graph_from_file("tests/fixtures/graphs/graph_cycle_outer.json");
        let scheduler = Scheduler::new(graph, None);

        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });

        let test_value = "foo";
        let test_calculated_value = "foxtrot foo bar";

        scheduler.url(
            "index".to_string(),
            serde_json::Value::String(test_value.to_string()),
            "field".to_string(),
        );

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        println!("{}", value);
        assert_eq!(value, test_calculated_value, "Expected to see another value here.");

    }

    #[tokio::test]
    async fn async_linked_three_cycle_graph() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let graph = load_graph_from_file("tests/fixtures/graphs/graph_three_cycle_step_one.json");
        let scheduler = Scheduler::new(graph, None);

        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            let _ = tx.try_send(event);
        });

        let test_value = "foo";
        let test_calculated_value = "oscar foo baz bar zaz";

        scheduler.url(
            "index".to_string(),
            serde_json::Value::String(test_value.to_string()),
            "field".to_string(),
        );

        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await.expect("Timeout waiting for event").expect("Channel closed unexpectedly");
        let value = event.data.get("return").and_then(serde_json::Value::as_str).expect("Could not find return key.");
        assert_eq!(value, test_calculated_value, "Expected to see another value here.");

    }

}
