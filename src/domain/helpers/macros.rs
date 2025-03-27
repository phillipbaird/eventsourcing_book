/// Macro for construction of ID types using the "new type" pattern, i.e. they wrap a UUID.
/// Support is provided for SQLx so these types can be used in database queries.
/// All IDs use v7 Uuids for more efficient database indexing.
#[macro_export]
macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
            sqlx::Type,
        )]
        #[sqlx(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            pub fn new() -> Self {
                $name(uuid::Uuid::now_v7())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl fake::Dummy<fake::Faker> for $name {
            fn dummy_with_rng<R: fake::Rng + ?Sized>(_config: &fake::Faker, rng: &mut R) -> Self {
                use fake::Fake;
                let uuid: uuid::Uuid = fake::uuid::UUIDv7.fake_with_rng(rng);
                $name(uuid)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let uuid = uuid::Uuid::from_str(s)?;
                Ok($name(uuid))
            }
        }

        impl TryFrom<uuid::Uuid> for $name {
            type Error = $crate::domain::cart::UuidNotCompatible;

            fn try_from(uuid: uuid::Uuid) -> Result<Self, Self::Error> {
                if let Some(version) = uuid.get_version() {
                    if version == uuid::Version::SortRand {
                        Ok($name(uuid))
                    } else {
                        Err($crate::domain::cart::UuidNotCompatible(uuid))
                    }
                } else {
                    Err($crate::domain::cart::UuidNotCompatible(uuid))
                }
            }
        }

        impl From<$name> for uuid::Uuid {
            fn from(value: $name) -> uuid::Uuid {
                value.0.clone()
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                use std::str::FromStr;
                $name::from_str(&value).unwrap()
            }
        }

        impl disintegrate::IntoIdentifierValue for $name {
            const TYPE: disintegrate::IdentifierType = disintegrate::IdentifierType::Uuid;
            fn into_identifier_value(self) -> disintegrate::IdentifierValue {
                self.0.into_identifier_value()
            }
        }
    };
}
