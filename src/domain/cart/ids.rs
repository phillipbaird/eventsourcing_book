use crate::{infra::ClientError, uuid_id};

uuid_id!(CartId);
uuid_id!(ItemId);
uuid_id!(ProductId);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Uuid {0} is not compatible. Version 7 Uuid is required.")]
pub struct UuidNotCompatible(pub uuid::Uuid);

impl From<UuidNotCompatible> for ClientError {
    fn from(value: UuidNotCompatible) -> Self {
        ClientError::Payload(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::domain::cart::ProductId;

    #[test]
    fn confirm_v7_uuid_can_be_converted_to_product_id() {
        let uuid = Uuid::now_v7();
        assert!(ProductId::try_from(uuid).is_ok())
    }

    #[test]
    fn confirm_v4_uuid_cannot_be_converted_to_product_id() {
        let uuid = Uuid::new_v4();
        assert!(ProductId::try_from(uuid).is_err())
    }
}
