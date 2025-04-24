# Understanding Event Sourcing

A Dynamic Consistency Boundry ("Kill the Aggregates") implementation of the Event Model from Martin Dilger's book *Understanding Event Sourcing* written in Rust.

---

## 📘 About the Book

*Understanding Event Sourcing* by Martin Dilger introduces the principles of Event Sourcing and Event Modeling, providing readers with practical insights and techniques to design and implement event-sourced systems.

- 📐 [Find the Event Model that is used as a basis for implementation](https://miro.com/app/board/uXjVKvTN_NQ=/)
- 📖 [Get the book on Leanpub](https://leanpub.com/eventmodeling-and-eventsourcing)
- 🖥️ [Check out the original repository](https://github.com/dilgerma/eventsourcing-book)

---

## Chapter Notes

### Chapter 21 - AddItem

In this implementation, each slice is implemented in it's own source file in [`/src/domain/cart`](https://github.com/phillipbaird/eventsourcing_book/tree/main/src/domain/cart). The Add Item slice can be found [here](https://github.com/phillipbaird/eventsourcing_book/blob/main/src/domain/cart/add_item.rs).

The AddItemCommand is a simple struct. The AddItemCommand implements the Decision trait which has a `process` method responsible for validating the command and either returning an error or one or more events. The the `process` method is supplied with some state which is used to validate the command. This state is derived from the events in the event store.

In this case the state for the command is implemented by a struct named AddItemState. It just identifies if the cart exists and how many items it holds. Later in chapter 27, a boolean flag was added to indicate if the cart has been submitted.

Which events are used to derive the state of the cart is controlled by a state query. In this case a derive macro has been used to automatically generate the query from the CartStream. The attribute `#[state_query(CartStream)]` identifies the stream and the `#[id]` field attribute identifies that we are only interested in events that match the cart id loaded into the AddItemState struct.

For a better introduction to what is happening here I'd recommend browsing the Disintegrate docs, especially the [introduction](https://disintegrate-es.github.io/disintegrate/) and explanation of [Decisions](https://disintegrate-es.github.io/disintegrate/decision).

At the end of the [`/src/domain/cart/add_item.rs` file](https://github.com/phillipbaird/eventsourcing_book/blob/main/src/domain/cart/add_item.rs) you will find the unit tests in a Given-When-Then format. Note in Rust it is normal practice to locate unit tests in the same file as the code they are testing. The `#[cfg(test)]` attribute on the `tests` sub-module ensures those tests do not end up in the release binary.

### Chapter 22 - Live Projections

A live projection is implemented in [`/src/domain/cart/cart_items.rs`](https://github.com/phillipbaird/eventsourcing_book/blob/main/src/domain/cart/cart_items.rs). The [`cart_items_read_model` function](https://github.com/phillipbaird/eventsourcing_book/blob/7afdbc5c098beb6ca18f8330b66bd8ad7dabcb81/src/domain/cart/cart_items.rs#L63-L69) does two things. First it creates an event query that will return the events needed for the read model. It then calls a helper function `read_from_events` which reads the events from the event store and does a left-fold to produce the read model. Note in this case we are returning the read model appended in an Option type. An Option type has two variants Some and None which allow us to indicate if the cart was found. The function named `apply_event` is our fold function. It takes a read model and an event and returns a new read model.

There are two styles of test shown. The first takes a Given-Then approach which simply validates the fold function used to produce the read model. The second test processes commands and then produces the read model from the events that were produced. Note the `#[sqlx::test]` annotation provides our test with a connection pool to a clean database. Nice. Some folks might think that because we're writing to the database this should be an integration test. I'd agree if this was slow, but it runs in milliseconds so I've kept it as a unit test.

### Chapter 23 - Remove Item & Clear Cart

Both the [Remove Item] and [Clear Cart] slices are very similar to the Add Item slice. See the Add Item notes above for an explanation of what is happening.

### Chapter 24 - Change Inventory

The Change Inventory slice needed to listen to messages from a Kafka topic. This required to build a little bit of infrastructure to listen to the required Kafka topic. That infrastructure can be found in [`/src/subsystems/kafka_listeners.rs`](https://github.com/phillipbaird/eventsourcing_book/blob/main/src/subsystems/kafka_listeners.rs). This infrastructure records what messages have already been processed so that old messages do not get reprocessed.

The implementation of the slice in [`/src/domain/cart/change_inventory.rs`](https://github.com/phillipbaird/eventsourcing_book/blob/main/src/domain/cart/change_inventory.rs) provides a KafkaMessageHandler for the "inventories" topic. This translates the message to a ChangeInventoryCommand which is then processed. There is no validation associated with the command.

In cases like this where a command has no validation the Disintegrate crate allows us to forgo processing a command and we can simply append the event to the event store using the [`append_without_validation` method](https://docs.rs/disintegrate/latest/disintegrate/trait.EventStore.html#tymethod.append_without_validation). However, I decided to stay aligned with the book and stuck with the Command-Event pattern. This highlighted an assumption in the Disintegrate crate that processing a commend requires some state to validate against. There is no state needed in this case so some dummy state  was created using an EmptyStream with an EmptyEvent. See the code comments in the [`stateless` helper module](https://github.com/phillipbaird/eventsourcing_book/blob/main/src/domain/helpers/stateless.rs) for more details.

### Chapter 25 - Inventories

The Inventories slice is implemented in `/src/domain/cart/inventories.rs`. The InventoriesReadModelProjection is implemented as a Disintegrate EventListener listening to the InventoryStream.

In this implementation we are not using an ORM but using SQL to a Postgresql database.
Note we track the last event processed in the read model. This is because Disintegrate Event Listeners are "at least once". We therefore need to ensure our projection does not reprocess events it has already seen.



### Chapter 26 - Implementing Automations

The "Eventual Consistency" section on page 411 discusses the problem of eventual consistency when archiving items.
"Make it immediately consistent" (starting on page 413) is the solution selected in the book.
The event sourcing crate we are using, Disintegrate, does not support immediately consistent read models so another solution was required.

The first step was to remove the race condition. We want the PriceChanged events to only archive items that should be archived. We do not want PriceChanged events to cause the archiving of items until we are certain all AddItem events that came before the PriceChanged event have been processed by the CartsWithProducts read model. Otherwise there is potential for an item that should have been archived to be missed. Likewise any AddItem events that came after the PriceChanged event should not be archived.

Because this implementation is not tied to the concept of aggregates our event listeners can listen to events from any stream, part of a stream, or combination of streams. So our solution in this case is for the CartsWithProducts read model projection to process events from the union of the Cart stream and Pricing stream. This has the effect of serialising the events so that a PriceChanged event is always guaranteed to be handled after all the AddItem events that came before it. Likewise any AddItem events that came after the PriceChanged event will not be archived as the CartsWithProducts read model remains static while items are are being archived.

This approach does have a compromise. We now have a projection/read-model implementation that triggers commands. This will be a problem if this read-model ever needs to be rebuilt via the projection, because we will not want those commands to be executed again. This is discussed in the book at the end of chapter 28. Axon 

### Chapter 27 - Submitting the Cart

> One of the things the scope of the book does not include is how should the behaviour we've built so far act when a cart has been submitted?  Based on the fact that you cannot submit a cart a second time, I've assumed the cart should become immutable. We should not be able to add and remove items from a submitted cart and price changes should not result in items being archived from submitted carts. So in this implementation you will find validation has been added to prevent modifying submitted carts.

#### Publishing the Cart

Publishing a cart requires sending a message to Kafka. This requires us to work with an external system so there is potential for availability problems and other failures. It is not ideal for event handlers to get blocked by potentially long running processors, so I decided to enable processors to be triggered by an event but run in the background.

To do this a work queue was implemented inspired by this [blog post](https://kerkour.com/rust-job-queue-with-postgresql) by Sylvain Kerkour. A Postgreql table stores tasks to be completed in the background. Batches of tasks are picked up from the queue and run concurrently. Individual tasks can be configured to retry on failure up to a certain number of times or for a maximum amount of time with exponential backoff.

In this case our Publish Cart Processor is a function that is placed in the work queue and given up to 1 hour to succeed (maybe Kafka's down for some reason?). Either the processor or the work queue can be responsible for recording an event on success or failure. For this processor success is recorded by the processor itself as it is done as part of a Kafka transaction. If after 1 hour the processor is still failing, an event is recorded for the failure by the work queue and the processor task is no longer retried.

### Chapter 28 - Breaking Changes

The Disintegrate crate that provides our event store does not have any built-in functionality to upcast events.
However via the Serde crate there are certain changes that can be accommodated when an event is deserialised. In this case I've used the ability to provide a default value via a function call for the new fingerprint field. This takes the form a field attribute like `#[serde(default = "function_name")]`. Unlike the Axon implementation the old and new versions of the event are not separate. This makes creating a similar upcasting test to that shown in this chapter as rather infeasible. The best we could do is test some JSON value for the old event properly deserialises with the new field. In effect recreating testing already done by the Serde crate. In this case all we need to do is be certain the function providing the default value, returns the correct value. The rest should take care of itself.

---

## Technical Notes

This project uses the [SQLx crate](https://github.com/launchbadge/sqlx/blob/main/README.md) for working with the database. It is also using compile-time validation of SQL queries, so in order to compile this project you need to have a database available to compile against.

The project also requires a Kafka instance as this is required for running the integration tests.

To simplify starting up a development environment, there is an [`init_dev.sh` script](https://github.com/phillipbaird/eventsourcing_book/blob/main/init_dev.sh) and [`docker-compose.yml` file](https://github.com/phillipbaird/eventsourcing_book/blob/main/docker-compose.yml). Running the script will setup the database and Kafka containers required for the project. The script requires the `psql`, `docker`, `docker-compose` and [`sqlx`](https://github.com/launchbadge/sqlx/blob/main/sqlx-cli/README.md) commands to be available.
