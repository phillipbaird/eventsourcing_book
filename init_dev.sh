#!/usr/bin/env bash
# Initialise the Dev environment.
# - starts Postgres and Kafka containers.
# - connects to the database and loads the database schema.
# - constucts the required Kafka topics.

set +x
set -eo pipefail

# Check required commands are installed
if ! [ -x "$(command -v psql)" ]; then
  echo >&2 "Error: $(psql) is not installed."
  exit 1
fi

if ! [ -x "$(command -v sqlx)" ]; then
  echo >&2 "Error: $(sqlx) is not installed."
  echo >&2 "Use:"
  echo >&2 "cargo install sqlx-cli --no-default-features --features postgres"
  echo >&2 "to install it."
  exit 1
fi

if ! [ -x "$(command -v docker-compose)" ]; then
  echo >&2 "Error: $(docker-compose) is not installed."
  exit 1
fi

# Check if a custom user has been set, otherwise default to 'postgres'
DB_USER=${POSTGRES_USER:=postgres}
# Check if a customer password has been set, otherwise default to 'password'
DB_PASSWORD="${DB_PASSWORD:=password}"
# Check if a custom data name has been set, otherwise default to 'myapp'
DB_NAME="${POSTGRESS_DB:=cart}"
# Check if a custom port has been set, otherwise default to '5432'
DB_PORT="${POSTGRES_PORT:=5432}"

# Start the containers.
docker-compose up -d

# Keep pinging Postgres until it's ready to accept commands
export PGPASSWORD="${DB_PASSWORD}"
until psql -h "localhost" -U "${DB_USER}" -p "${DB_PORT}" -d "postgres" -c '\q' 2>/dev/null; do
  >&2 echo "Postgres is not yet available - sleeping"
  sleep 1
done

>&2 echo "Postgres is up and running on port ${DB_PORT}!"

export DATABASE_URL=postgres://${DB_USER}:${DB_PASSWORD}@localhost:${DB_PORT}/${DB_NAME}
echo $DATABASE_URL
sqlx database create
sqlx migrate run --source migrations

# cargo sqlx prepare --workspace -- --features ssr

>&2 echo "You can access the database via psql -h localhost -U ${DB_USER} -p ${DB_PORT} -d ${DB_NAME}"

# Create/recreate the Kafka topics.
docker exec -it cart_kafka /opt/bitnami/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --delete --if-exists --topic inventories
docker exec -it cart_kafka /opt/bitnami/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --create --topic inventories
docker exec -it cart_kafka /opt/bitnami/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --delete --if-exists --topic price-changes
docker exec -it cart_kafka /opt/bitnami/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --create --topic price-changes
docker exec -it cart_kafka /opt/bitnami/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --delete --if-exists --topic published-carts
docker exec -it cart_kafka /opt/bitnami/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --create --topic published-carts
