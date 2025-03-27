use disintegrate::{StateMutate, StateQuery};

use crate::domain::EmptyStream;

/// Stateless can be used as a StateQuery that will never find any state.
/// It can be used when you need to process a command that does not need to read any
/// existing state in order to make a Decision. This should be a rarity.
/// The Disintegrate crate (as at version 2.0) assumes all Decisions need to read some state.
/// Stateless can be used as a StateQuery to work around this assumption.
/// For example, in the change_inventory slice, we receive an external event from another system and
/// translate it into an internal event which is later used to update a view model of the inventory.
/// The external event we receive is idempotent and does not require any validation.
/// We use Stateless as the StateQuery of the command to fulfill the requirements of the Decision
/// trait.
/// There is an EventStore.append_without_validation(...) method which allows events to be
/// appended to the event store, but in the case of these "translator" processes I chose to stick
/// with a Command -> Event coding pattern.
#[derive(Clone, Debug, PartialEq, Eq, StateQuery, serde::Serialize, serde::Deserialize)]
#[state_query(EmptyStream)]
pub struct Stateless;

impl StateMutate for Stateless {
    fn mutate(&mut self, _event: Self::Event) {}
}
