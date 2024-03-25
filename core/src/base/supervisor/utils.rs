// fn to_serialized_event<Event: DomainEvent>(
//     app_id: impl Into<String>,
//     event: &EventEnvelope<Event>,
// ) -> SerializedEvent {
//     SerializedEvent {
//         aggregate_id: event.aggregate_id.clone(),
//         sequence: event.sequence.clone(),
//         payload: serde_json::to_value(event.payload.clone()).expect("Error deserializing Value"),
//         metadata: serde_json::to_value(event.metadata.clone()).expect("Error deserializing Value"),
//         event_type: event.payload.event_type(),
//         event_version: event.payload.event_version(),
//         app_id: app_id.into(),
//     }
// }

// #[async_trait]
// impl<'query, E> Query<E> for QueryBridge<'query, E>
// where
//     E: DomainEvent,
// {
//     async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<E>]) {
//         let events: Vec<SerializedEvent> = events
//             .iter()
//             .map(|x| to_serialized_event(&self.app_id, x.into()))
//             .collect();

//         self.inner.dispatch(aggregate_id, &events);
//     }
// }
// //
