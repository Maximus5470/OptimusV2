# Java Execution Environment
FROM eclipse-temurin:17-jdk-alpine

WORKDIR /code

# Install JUnit for testing
RUN apk add --no-cache wget && \
    wget https://repo1.maven.org/maven2/junit/junit/4.13.2/junit-4.13.2.jar -P /opt/ && \
    wget https://repo1.maven.org/maven2/org/hamcrest/hamcrest-core/1.3/hamcrest-core-1.3.jar -P /opt/ && \
    apk del wget

ENV CLASSPATH=/opt/junit-4.13.2.jar:/opt/hamcrest-core-1.3.jar

# Copy and compile runner script
COPY dockerfiles/java/Runner.java /Runner.java
RUN javac /Runner.java

# Default command runs the runner
CMD ["java", "Runner"]
