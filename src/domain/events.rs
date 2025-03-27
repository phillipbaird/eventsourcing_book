use super::{cart::*, helpers::device_fingerprint_calculator::default_fingerprint};
use rust_decimal::Decimal;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize, disintegrate::Event)]
#[stream(CartStream, [CartCreated, CartItemAdded, CartItemRemoved, CartCleared, ItemArchivedEvent, CartSubmitted])]
#[stream(EmptyStream, [EmptyEvent])]
#[stream(InventoryStream, [InventoryChanged])]
#[stream(PricingStream, [PriceChanged])]
#[stream(PublishedStream, [CartPublished, CartPublicationFailed])]
#[stream(SubmittedStream, [CartSubmitted])]
pub enum DomainEvent {
    CartCleared {
        #[id]
        cart_id: CartId,
    },
    CartCreated {
        #[id]
        cart_id: CartId,
    },
    CartItemAdded {
        #[id]
        cart_id: CartId,
        description: String,
        image: PathBuf,
        price: Decimal,
        #[id]
        item_id: ItemId,
        #[id]
        product_id: ProductId,
        #[serde(default = "default_fingerprint")]
        fingerprint: String,
    },
    CartItemRemoved {
        #[id]
        cart_id: CartId,
        #[id]
        item_id: ItemId,
    },
    CartPublished {
        #[id]
        cart_id: CartId,
    },
    CartPublicationFailed {
        #[id]
        cart_id: CartId,
    },
    CartSubmitted {
        #[id]
        cart_id: CartId,
        ordered_product: Vec<OrderedProduct>,
        total_price: Decimal,
    },
    EmptyEvent,
    InventoryChanged {
        #[id]
        product_id: ProductId,
        inventory: i32,
    },
    ItemArchivedEvent {
        #[id]
        cart_id: CartId,
        #[id]
        item_id: ItemId,
        price_changed_event_id: i64,
    },
    PriceChanged {
        #[id]
        product_id: ProductId,
        old_price: Decimal,
        new_price: Decimal,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct OrderedProduct {
    pub product_id: ProductId,
    pub price: Decimal,
}
