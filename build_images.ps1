
# Build language images for Optimus
docker build -t optimus-python:latest -f dockerfiles/python/Dockerfile .
docker build -t optimus-java:latest -f dockerfiles/java/Dockerfile .
docker build -t optimus-rust:latest -f dockerfiles/rust/Dockerfile .
