# Build the Spring Boot fat jar with Java 25, then run it on a slim JRE.
FROM maven:3.9-eclipse-temurin-25@sha256:01ef98a139ed64622c086bac54d1e167453d0f2ff68b69d00978f26d8736215c AS build
WORKDIR /src
# Resolve dependencies first so they cache across source-only changes.
COPY pom.xml .
RUN mvn -q -B -e dependency:go-offline
COPY src ./src
RUN mvn -q -B -e -DskipTests package

FROM eclipse-temurin:25-jre@sha256:04262e8782d6b034ee5d7c1c5d4e8938fcf2063a76b4bfcd84e5d994d09c27bc
WORKDIR /app
COPY --from=build /src/target/*.jar app.jar
EXPOSE 8080
ENTRYPOINT ["java", "-jar", "app.jar"]
