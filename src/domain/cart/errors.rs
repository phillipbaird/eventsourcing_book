use super::CartId;

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum CartError {
    #[error("CartID {0} is not unique.")]
    IdConsumed(CartId),
    #[error("Cart with ID {0} does not exist.")]
    CartDoesNotExist(CartId),
    #[error("Cannot add item. Cart is full (max 3 items).")]
    CannotAddItemCartFull,
    #[error("Cannot remove item. Item not in cart.")]
    CannotRemoveItem,
    #[error("Cannot submit an empty cart.")]
    CannotSubmitEmptyCart,
    #[error("Cannot submit cart twice.")]
    CannotSubmitCartTwice,
    #[error("Cart has been submitted. Cannot be altered.")]
    CartCannotBeAltered,
}
