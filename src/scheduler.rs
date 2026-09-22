use rusty_v8 as v8;
use std::sync::{Arc, Once};
use uuid::Uuid;
use crate::utils::{serde_json_to_v8, v8_value_to_serde_json, log, increment_sequence_counter};
use crate::loader::{get_graph_from_global_store, file};
use crate::instances::{get_instance, instance_for, set_instance_state, DEPTH_LIMIT};
use crate::types::*;
use crate::event_emitter::EVENT_EMITTERS;

static V8_INIT: Once = Once::new();

impl Scheduler {
    pub fn new(graph: Graph, scheduler_id: Option<String>) -> Self {
        Scheduler::new_in_instance(graph, scheduler_id, String::new())
    }

    /// The same scheduler, running inside one use of a linked graph.
    pub fn new_in_instance(graph: Graph, scheduler_id: Option<String>, instance_key: String) -> Self {
        Scheduler::initialize_v8();
        let id = scheduler_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut event_emitters = EVENT_EMITTERS.lock().expect("Could not lock global event emitter hash map");
        let emitter = event_emitters.entry(id.clone())
            .or_insert_with(|| Arc::new(EventEmitter::new()))
            .clone();

        Self {
            event_emitter: emitter,
            graph,
            id,
            instance_key,
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
        // Define a setter for each edge field.  One accessor per *field*, not
        // per connector: the loop used to build a whole new edges object for
        // every connector and put it in the global, so only the last connector
        // of the last edge was ever reachable — fan-out went to one place, and
        // a node with two edges lost the first (RT-26).
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
            connector_count += edge.connectors.len();
            let setter =
                |scope: &mut v8::HandleScope<'_>,
                 key: v8::Local<'_, v8::Name>,
                 value: v8::Local<'_, v8::Value>,
                 args: v8::PropertyCallbackArguments<'_>| {
                    let this = args.this();
                    let edge_field = key.to_rust_string_lossy(scope);
                    let read = |scope: &mut v8::HandleScope<'_>, name: &str| -> String {
                        let key = v8::String::new(scope, name).expect("Failed to read edge property").into();
                        this.get(scope, key)
                            .and_then(|v| v.to_string(scope))
                            .map(|s| s.to_rust_string_lossy(scope))
                            .unwrap_or_default()
                    };
                    let scheduler_id = read(scope, "schedulerId");
                    let node_id = read(scope, "nodeId");
                    let graph_id = read(scope, "graphId");
                    let field = read(scope, "field");
                    let instance_key = read(scope, "instanceKey");
                    let connectors_json = read(scope, &format!("connectors:{}", edge_field));
                    let connectors: Vec<Connector> = serde_json::from_str(&connectors_json).unwrap_or_default();
                    let sent = v8_value_to_serde_json(value, scope);
                    let emitter = {
                        let emitters = EVENT_EMITTERS.lock().expect("Could not lock global event emitter hash map");
                        emitters.get(&scheduler_id).cloned()
                    };
                    // every connector on this field, in order: a hyperedge
                    // delivers to all of them, not to the last one written
                    for connector in connectors {
                        increment_sequence_counter();
                        if let Some(emitter) = &emitter {
                            emitter.emit(Event {
                                event_type: EventType::BeginConnector,
                                data: serde_json::json!({
                                  "graphId": graph_id,
                                  "nodeId": node_id,
                                  "connectorId": connector.id,
                                  "connectorField": connector.field,
                                  "connectorNodeId": connector.node_id,
                                  "connectorVersion": connector.version,
                                  "connectorGraphId": connector.graph_id,
                                  "field": field,
                                  "edgeField": edge_field,
                                  "value": sent,
                                }),
                            });
                        }
                        log("edge connector invoke", format!("graph_id: {}, from_node_id: {}, to_node_id: {}, edge_field: {}, to_field: {}, value: {:?}, scheduler_id: {}", graph_id, node_id, connector.node_id, edge_field, connector.field, sent, scheduler_id));
                        /*
                         * Where this value is going.  Inside an instance the
                         * connectors are that instance's, so it stays there; a
                         * connector naming another graph is on its way back out
                         * to whichever instance called this one, and that copy
                         * is in memory — the global store holds documents, and
                         * delivering to a document would be delivering to the
                         * wrong copy.
                         */
                        let (next_graph, next_instance) = if instance_key.is_empty() {
                            match get_graph_from_global_store(&connector.graph_id) {
                                Some(graph) => (graph, String::new()),
                                None => {
                                    log("edge", format!("Could not load next graph. graph_id: {}", connector.graph_id));
                                    continue;
                                }
                            }
                        } else {
                            let instance = get_instance(&instance_key).expect(&format!("Could not find instance {}", instance_key));
                            if connector.graph_id == instance.graph.id {
                                (instance.graph.clone(), instance_key.clone())
                            } else {
                                match instance.parent_key.clone() {
                                    Some(parent_key) => {
                                        let parent = get_instance(&parent_key).expect(&format!("Could not find instance {}", parent_key));
                                        (parent.graph.clone(), parent_key)
                                    }
                                    None => {
                                        match get_graph_from_global_store(&connector.graph_id) {
                                            Some(graph) => (graph, String::new()),
                                            None => {
                                                log("edge", format!("Could not load next graph. graph_id: {}", connector.graph_id));
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                        };
                        let scheduler = Scheduler::new_in_instance(next_graph, Some(scheduler_id.clone()), next_instance);
                        scheduler.execute_node_by_id(
                            connector.node_id.clone(),
                            sent.clone(),
                            connector.field.clone(),
                        );
                        if let Some(emitter) = &emitter {
                            emitter.emit(Event {
                                event_type: EventType::EndConnector,
                                data: serde_json::json!({
                                  "graphId": graph_id,
                                  "nodeId": node_id,
                                  "connectorId": connector.id,
                                  "connectorField": connector.field,
                                  "connectorNodeId": connector.node_id,
                                  "connectorVersion": connector.version,
                                  "connectorGraphId": connector.graph_id,
                                  "field": field,
                                  "edgeField": edge_field,
                                  "value": sent,
                                }),
                            });
                        }
                    }
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

        /*
         * One edges object, holding what every setter needs to find its way.
         * A V8 callback cannot close over a Rust value, so what the setter
         * needs is written here as plain properties and read back through
         * `this`.  The same values go on the global for node code to use — a
         * node with no edges at all still expects `field` and `nodeId` to be
         * there.
         */
        let global = scope.get_current_context().global(scope);
        let edges_object: v8::Local<'_, v8::Object> = object_template.new_instance(scope).expect("Could not create v8 object template");
        for obj in [edges_object, global] {
            Scheduler::set_key_value(scope, "schedulerId", &self.id.to_string(), obj);
            Scheduler::set_key_value(scope, "nodeId", &node.id, obj);
            Scheduler::set_key_value(scope, "graphId", &self.graph.id, obj);
            Scheduler::set_key_value(scope, "field", &field, obj);
            Scheduler::set_key_value(scope, "instanceKey", &self.instance_key, obj);
        }
        for edge in &node.edges {
            let connectors = serde_json::to_string(&edge.connectors).unwrap_or_else(|_| "[]".to_string());
            Scheduler::set_key_value(scope, &format!("connectors:{}", edge.field), &connectors, edges_object);
        }
        global.set(scope, edges_key.into(), edges_object.into()).expect("Could not set edges object into global scope");
        let _ = connector_count;

        /*
         * State belonging to this use of the subgraph.  Every node here runs
         * in an isolate of its own, so the state is handed in as a value and
         * taken back out after the code has run — which is what makes it
         * *this* instance's state rather than a global everything shares.
         */
        let instance = if self.instance_key.is_empty() { None } else { get_instance(&self.instance_key) };
        let state_value = match &instance {
            Some(instance) => instance.state.clone(),
            None => serde_json::json!({}),
        };
        let state_key = v8::String::new(scope, "state").expect("Could not create state key");
        let state_v8 = serde_json_to_v8(scope, &state_value);
        global.set(scope, state_key.into(), state_v8.into()).expect("Could not set state into global scope");
        if let Some(instance) = &instance {
            let instance_object: v8::Local<'_, v8::Object> = object_template.new_instance(scope).expect("Could not create v8 object template");
            Scheduler::set_key_value(scope, "path", &instance.path.join("/"), instance_object);
            Scheduler::set_key_value(scope, "depth", &instance.depth.to_string(), instance_object);
            let instance_key_v8 = v8::String::new(scope, "instance").expect("Could not create instance key");
            global.set(scope, instance_key_v8.into(), instance_object.into()).expect("Could not set instance into global scope");
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
            // what the node left in `state` belongs to this instance
            if !self.instance_key.is_empty() {
                let global_after = try_catch.get_current_context().global(try_catch);
                let state_name = v8::String::new(try_catch, "state").expect("Could not create state key");
                if let Some(state_after) = global_after.get(try_catch, state_name.into()) {
                    set_instance_state(&self.instance_key, v8_value_to_serde_json(state_after, try_catch));
                }
            }
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
                /*
                 * A node carrying another graph is a call.  The instance it
                 * belongs to is named by the host nodes it was reached
                 * through, so the same use keeps its state between calls, two
                 * uses share nothing, and a graph reached through itself is a
                 * deeper path — a new instance, with its own everything.
                 */
                if node.linked_graph.is_some() {
                    let host = node.clone();
                    let linked = host.linked_graph.clone().expect("Linked graph disappeared between checks");
                    let load = |source: &str| -> Option<Graph> {
                        if source.is_empty() {
                            return None;
                        }
                        get_graph_from_global_store(source).or_else(|| Some(file(source)))
                    };
                    match instance_for(&host, Some(self.instance_key.as_str()), &self.graph.id, &load, DEPTH_LIMIT) {
                        Ok(instance) => {
                            // the field the host was given names the node inside that starts the work
                            let (inner_id, inner_field) = match linked.fields.inputs.get(&field) {
                                Some(mapped) => (mapped.id.clone(), mapped.field.clone()),
                                None => match linked.fields.inputs.values().next() {
                                    Some(only) => (only.id.clone(), only.field.clone()),
                                    None => {
                                        log("linked graph", format!("node {} has no way in for field {}", host.id, field));
                                        return;
                                    }
                                },
                            };
                            let inner = Scheduler::new_in_instance(instance.graph.clone(), Some(self.id.clone()), instance.key());
                            inner.execute_node_by_id(inner_id, value, inner_field);
                        }
                        Err(reason) => {
                            log("linked graph", reason.clone());
                            self.event_emitter.emit(Event {
                                event_type: EventType::Error,
                                data: serde_json::json!({
                                    "graphId": self.graph.id,
                                    "nodeId": host.id,
                                    "field": field,
                                    "value": value,
                                    "error": reason,
                                }),
                            });
                        }
                    }
                    return;
                }
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
    use crate::loader::file;
    use crate::utils::set_verbosity;
    use crate::instances::{reset_instances, instance_keys, instance_count};
    use std::sync::Mutex;
    use super::*;

    #[test]
    fn minimal_viable_graph() {
        let graph = file("tests/fixtures/graphs/graph_minimal.json");
        let scheduler = Scheduler::new(graph, None);
        set_verbosity(u32::max_value());
        assert_eq!(scheduler.graph.url, "foo", "The graph url did not match 'foo'");
    }

    #[tokio::test]
    async fn single_node_js_invoke() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let graph = file("tests/fixtures/graphs/graph_with_one_js_test.json");
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
        let graph = file("tests/fixtures/graphs/graph_with_edge.json");
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
        let graph = file("tests/fixtures/graphs/graph_with_two_edges.json");
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
        assert_eq!(value, "End of the line. Data processed in Node3.", "Expected to see another value here.");
    }

    #[tokio::test]
    async fn async_graph_with_two_edges_then_error() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let graph = file("tests/fixtures/graphs/graph_with_two_edges_then_error.json");
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

        let graph = file("tests/fixtures/graphs/graph_linked.json");
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
        assert_eq!(value, test_return_value, "Expected to see another value here.");

    }

    #[tokio::test]
    async fn async_linked_cycle_graph() {
        reset_instances();
        let graph = file("tests/fixtures/graphs/graph_cycle_outer.json");
        let scheduler = Scheduler::new(graph, None);
        let seen = collect_returns(&scheduler);

        let test_value = "foo";
        let test_calculated_value = "foxtrot foo bar";

        scheduler.url(
            "index".to_string(),
            serde_json::Value::String(test_value.to_string()),
            "field".to_string(),
        );

        let returns = seen.lock().expect("Could not lock the return sink.").clone();
        assert!(returns.iter().any(|r| r == test_calculated_value),
            "expected {} to come out of the chain, saw {:?}", test_calculated_value, returns);

    }

    /*
     * Linked graphs as calls.  These are the three claims: a graph can contain
     * itself and each turn is its own, two uses of one subgraph share nothing,
     * and a recursion that does not stop itself is stopped with its path named.
     */
    fn collect_returns(scheduler: &Scheduler) -> Arc<Mutex<Vec<String>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        scheduler.event_emitter.subscribe(EventType::AfterSet, move |event| {
            if let Some(value) = event.data.get("return").and_then(serde_json::Value::as_str) {
                sink.lock().expect("Could not lock the return sink.").push(value.to_string());
            }
        });
        seen
    }

    #[tokio::test]
    async fn a_graph_that_contains_itself_runs_one_level_at_a_time() {
        reset_instances();
        let graph = file("tests/fixtures/graphs/graph_recursive.json");
        let scheduler = Scheduler::new(graph, None);
        let seen = collect_returns(&scheduler);
        scheduler.url("index".to_string(), serde_json::json!(3), "in".to_string());
        let returns = seen.lock().expect("Could not lock the return sink.").clone();
        // Delivery is synchronous here, so a node's own "afterSet" arrives
        // once everything it set in motion has finished: the deepest turn
        // answers first, like any recursive call returning.
        assert_eq!(returns, vec![
            "depth 3 value 0 visits 1",
            "depth 2 value 1 visits 1",
            "depth 1 value 2 visits 1",
            "depth 0 value 3 visits 1",
        ], "each turn should be a level of its own, and count only its own visit");
        // one instance per level, named by the hosts it was reached through
        assert_eq!(instance_keys(), vec!["self", "self/self", "self/self/self"]);
    }

    #[tokio::test]
    async fn the_same_use_keeps_its_state_between_calls() {
        reset_instances();
        let graph = file("tests/fixtures/graphs/graph_two_uses.json");
        let scheduler = Scheduler::new(graph, None);
        let seen = collect_returns(&scheduler);
        scheduler.url("index".to_string(), serde_json::json!("one"), "in".to_string());
        scheduler.url("index".to_string(), serde_json::json!("two"), "in".to_string());
        let returns = seen.lock().expect("Could not lock the return sink.").clone();
        // the entry node's own return comes after the two it fed
        let from_uses: Vec<String> = returns.into_iter().filter(|r| r.contains("called")).collect();
        assert_eq!(from_uses, vec![
            "first called 1 with one",
            "second called 1 with one",
            "first called 2 with two",
            "second called 2 with two",
        ], "each use should count its own calls and carry them between calls");
        assert_eq!(instance_keys(), vec!["first", "second"], "two uses, two instances");
    }

    #[tokio::test]
    async fn a_recursion_that_does_not_stop_itself_is_stopped_with_its_path() {
        reset_instances();
        let graph = file("tests/fixtures/graphs/graph_runaway.json");
        let scheduler = Scheduler::new(graph, None);
        let errors = Arc::new(Mutex::new(Vec::new()));
        let sink = errors.clone();
        scheduler.event_emitter.subscribe(EventType::Error, move |event| {
            if let Some(error) = event.data.get("error").and_then(serde_json::Value::as_str) {
                sink.lock().expect("Could not lock the error sink.").push(error.to_string());
            }
        });
        scheduler.url("index".to_string(), serde_json::json!(0), "in".to_string());
        let seen = errors.lock().expect("Could not lock the error sink.").clone();
        assert_eq!(seen.len(), 1, "the runaway should be stopped exactly once");
        assert!(seen[0].contains(&format!("Linked graphs went {} deep (limit {})", DEPTH_LIMIT + 1, DEPTH_LIMIT)),
            "the error should say how deep it went: {}", seen[0]);
        assert!(seen[0].contains("self -> self"), "the error should name the path: {}", seen[0]);
        assert_eq!(instance_count(), DEPTH_LIMIT, "it should stop at the ceiling, not past it");
    }

    #[tokio::test]
    async fn an_edge_delivers_to_every_connector_on_it() {
        reset_instances();
        let graph = file("tests/fixtures/graphs/graph_fanout.json");
        let scheduler = Scheduler::new(graph, None);
        let seen = collect_returns(&scheduler);
        scheduler.url("index".to_string(), serde_json::json!("v"), "in".to_string());
        let returns = seen.lock().expect("Could not lock the return sink.").clone();
        // An edge is a hyperedge: every connector on it gets the value.  The
        // edges object used to be rebuilt per connector and put in the global,
        // so only the last connector of the last edge could be reached at all.
        assert!(returns.iter().any(|r| r == "left got v"), "left never ran: {:?}", returns);
        assert!(returns.iter().any(|r| r == "right got v"), "right never ran: {:?}", returns);
        assert!(returns.iter().any(|r| r == "third got v again"), "the second edge never ran: {:?}", returns);
    }

    /*
     * The old arrangement, kept for anyone who wants one set of nodes: two
     * faults it carried for years, now covered.
     */
    #[test]
    fn flattening_points_connectors_at_the_inner_node_the_host_named() {
        reset_instances();
        let flat = crate::loader::flatten(&file("tests/fixtures/graphs/graph_cycle_outer.json"));
        let entry = flat.nodes.iter().find(|n| n.id == "1").expect("no entry node");
        let connector = &entry.edges[0].connectors[0];
        assert_eq!(connector.node_id, "86c4e1de-adb2-4376-8938-f0ed4432a550",
            "the connector should point at the inner node the host's input names");
        assert_eq!(connector.field, "charlie");
    }

    #[test]
    fn flattening_keeps_going_past_an_edge_it_does_not_recognise() {
        reset_instances();
        let flat = crate::loader::flatten(&file("tests/fixtures/graphs/graph_three_cycle_step_one.json"));
        // every graph in the chain contributed its nodes; the walk used to end
        // at the first edge whose field was not the output it was looking for
        assert!(flat.nodes.len() >= 5, "expected the whole chain, got {} nodes", flat.nodes.len());
    }

    #[tokio::test]
    async fn async_linked_three_cycle_graph() {
        reset_instances();
        let graph = file("tests/fixtures/graphs/graph_three_cycle_step_one.json");
        let scheduler = Scheduler::new(graph, None);
        let seen = collect_returns(&scheduler);

        let test_value = "foo";
        let test_calculated_value = "oscar foo baz bar zaz";

        scheduler.url(
            "index".to_string(),
            serde_json::Value::String(test_value.to_string()),
            "field".to_string(),
        );

        let returns = seen.lock().expect("Could not lock the return sink.").clone();
        assert!(returns.iter().any(|r| r == test_calculated_value),
            "expected {} to come out of the chain, saw {:?}", test_calculated_value, returns);

    }

}
