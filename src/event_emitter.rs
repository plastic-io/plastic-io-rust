use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use crate::types::*;

type GlobalEventEmitters = Arc<Mutex<HashMap<String, Arc<EventEmitter>>>>;

lazy_static! {
    pub static ref EVENT_EMITTERS: GlobalEventEmitters = Arc::new(Mutex::new(HashMap::new()));
}

/// `EventEmitter` is a thread-safe structure that allows for registering callbacks
/// to specific event types and emitting events to invoke these callbacks.
///
/// Each `EventEmitter` instance maintains its own internal registry of event
/// subscribers. Callbacks for each event type are executed in separate threads
/// when an event is emitted, ensuring concurrent execution without blocking the
/// emitter.
///
/// # Examples
///
/// Creating an `EventEmitter` and subscribing to an event:
///
/// ```
/// use plastic_io::types::{EventEmitter, EventType};
/// let emitter = EventEmitter::new();
/// emitter.subscribe(EventType::AfterSet, |event| {
///     println!("Event received: {:?}", event);
/// });
/// ```
///
/// Emitting an event:
///
/// ```
/// use plastic_io::types::{EventEmitter, Event, EventType};
/// let emitter = EventEmitter::new();
/// emitter.emit(Event {
///   event_type: EventType::BeginEdge,
///   data: serde_json::json!({
///     "data": "value",
///   }),
/// });
/// ```
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
