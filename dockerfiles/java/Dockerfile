# Java Execution Environment
FROM openjdk:17-slim

WORKDIR /code

# Install JUnit for testing
RUN apt-get update && \
    apt-get install -y wget && \
    wget https://repo1.maven.org/maven2/junit/junit/4.13.2/junit-4.13.2.jar -P /opt/ && \
    wget https://repo1.maven.org/maven2/org/hamcrest/hamcrest-core/1.3/hamcrest-core-1.3.jar -P /opt/ && \
    rm -rf /var/lib/apt/lists/*

ENV CLASSPATH=/opt/junit-4.13.2.jar:/opt/hamcrest-core-1.3.jar

# Default command (will be overridden)
CMD ["java", "--version"]
