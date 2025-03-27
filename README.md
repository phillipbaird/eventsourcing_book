Chapter 26 - Implementing Automations

The "Eventual Consistency" section on page 411 discusses the problem of eventual consistency when archiving items.
"Make it immediately consistent" (starting on page 413) is the solution selected in the book.
The event sourcing crate we are using, Disintegrate, does not support immediately consistent read models so another solution was required.

The first step was to remove the race condition. We want the PriceChanged events to only archive items that should be archived. We do not want PriceChanged events to cause the archiving of items until we are certain all AddItem events that came before the PriceChanged event have been processed by the CartsWithProducts read model. Otherwise there is potential for an item that should have been archived to be missed. Likewise any AddItem events that came after the PriceChanged event should not be archived.

Because this implementation is not tied to the concept of aggregates our event listeners can listen to events from any stream. So our solution in this can is for the CartsWithProducts read model projection to process events from the union of the Cart stream and Pricing stream. This has the effect of serialising the events so that a PriceChanged event is always guaranteed to be handled after all the AddItem events that came before it. Likewise any AddItem events that came after the PriceChanged event will not be archived as the CartsWithProducts read model remains static while items are are being archived.

This approach does have a compromise. We now have a projection/read-model implementation that triggers commands. This will be a problem if this read-model ever needs to be rebuilt via the projection, because we will not want those commands to be executed again. This is discussed in the book at the end of chapter 28.


Chapter 27 - Submitting the Cart

Sidenote: One of the things the scope of the book does not include is how should the behaviour we've built so far act when a cart has been submitted?
Based on the fact that you cannot submit a cart a second time, I've assumed the cart should become immutable. We should not be able to add and remove items from a submitted cart and price changes should not result in items being archived from submitted carts. So in this implementation you will find I have updated previous slices from earlier chapters to add validation and report an error if trying to modifying a submitted cart.

Publishing the Cart

Publishing a cart requires sending a message to Kafka. Working with external systems and services means there is potential for those systems to be unavailable.
I wanted to experiment with how to handle these sorts of problems failures and retries and see how that fits in with processors.


Chapter 28 - Breaking Changes

The Disintegrate crate that provides our event store does not have any built-in functionality to upcast events.
However via the Serde crate there are certain changes that can be accommodated when an event is deserialised. In this case I've used the ability to provide a default value via a function call for the new fingerprint field. This takes the form a field attribute like `#[serde(default = "function_name")]`. Unlike the Axon implementation the old and new versions of the event are not separate. This makes creating a similar upcasting test to that shown in this chapter as rather infeasible. The best we could do is test some json value for the old event properly deserialises with the new field. In effect recreating testing already done by the Serde crate. In this case all we need to do is be certain the function providing the default value, returns the correct value. The rest should take care of itself. 




Start Server - `cargo run`
Run unit tests - `cargo test --lib`
Run all tests - `cargo test`

