---
title: "Run Marreta in a container"
category: tutorials
slug: "tutorials/run-in-a-container"
summary: "Package a Marreta project as a container image and run it with Docker, Docker Compose, and Kubernetes."
---

# Run Marreta in a container

This tutorial packages a Marreta project as a container image and runs it three ways:
with Docker, with Docker Compose, and on Kubernetes. The app is stateless on purpose, so
the focus stays on containerizing and running the service. Adding a database or other
provider is a later step, linked at the end.

## Prerequisites

- You have finished the [Quickstart](quickstart.md), so the `marreta` CLI is installed on
  your machine.
- Docker is installed and running, with Docker Compose available (it ships with Docker
  Desktop and the current Docker CLI as `docker compose`).
- For the Kubernetes section, a cluster you can reach with `kubectl`. This tutorial assumes
  the cluster already exists and does not set one up.

## Create the project

Scaffold a fresh project with the CLI you installed in the Quickstart:

```bash
# Create a new project named hello-container
marreta init hello-container

# Move into the project directory
cd hello-container
```

The scaffold already includes a working route, `GET /greetings`, that returns a JSON
greeting. That is all you need to containerize.

## 1. Build and run with Docker

Marreta does not publish a container image yet, so you build your own. The image downloads
the Linux runtime binary from the latest GitHub release in a first stage, then copies just
that binary into a clean second stage along with your project. The two stages keep the
download tooling out of the final image.

Create a file named `Dockerfile` in the project root:

```dockerfile
# Stage 1: download the Marreta runtime binary for Linux
FROM ubuntu:24.04 AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://github.com/tm-dev-lab/marreta-lang/releases/latest/download/marreta-linux-x86_64 \
    -o /marreta \
    && chmod +x /marreta

# Stage 2: the runtime image with your project
FROM ubuntu:24.04
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /marreta /usr/local/bin/marreta
WORKDIR /app
COPY . /app/
EXPOSE 8080
CMD ["marreta", "serve"]
```

The asset `marreta-linux-x86_64` is the Intel and AMD build. On an arm64 host or cluster,
use `marreta-linux-arm64` instead. To pin a version rather than tracking the latest
release, replace `releases/latest/download` with `releases/download/v0.2.0`, using the
release tag you want.

Now build the image and run it:

```bash
# Build the image and tag it hello-container:local
docker build -t hello-container:local .

# Run it, mapping the container port 8080 to the same port on your machine
docker run --rm -p 8080:8080 hello-container:local
```

In another terminal, call the route:

```bash
# The scaffolded route returns a JSON greeting
curl localhost:8080/greetings
# {"message":"Hello, Marreta!"}
```

Stop the container with `Ctrl+C`.

As your project grows, add a `.dockerignore` so local files like `marreta.env` stay out of
the image, since configuration belongs to each environment (see
[Configure environment variables](../how-to/configure-environment.md)).

### Alternative: download the binary yourself

If you would rather fetch the binary outside the build, for example to cache or scan it
first, download it next to the `Dockerfile`:

```bash
# Download the Linux runtime binary into the project
curl -fsSL https://github.com/tm-dev-lab/marreta-lang/releases/latest/download/marreta-linux-x86_64 -o marreta

# Make it executable
chmod +x marreta
```

Then use a single-stage `Dockerfile` that copies the binary you already downloaded instead
of fetching it:

```dockerfile
FROM ubuntu:24.04
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY marreta /usr/local/bin/marreta
WORKDIR /app
COPY . /app/
EXPOSE 8080
CMD ["marreta", "serve"]
```

Build and run it the same way as above.

## 2. Run with Docker Compose

Compose is handy once you add dependencies later. This step runs the image you built in
section 1, so make sure that `docker build` finished successfully first.

Create a file named `compose.yaml` in the project root, next to the `Dockerfile`:

```yaml
services:
  api:
    image: hello-container:local
    ports:
      - "8080:8080"
```

Bring it up, call it, and tear it down:

```bash
# Start the service in the background, using the image you built above
docker compose up -d

# Call the route
curl localhost:8080/greetings
# {"message":"Hello, Marreta!"}

# Stop and remove the service
docker compose down
```

## 3. Run on Kubernetes

This section assumes you already have a cluster and that `kubectl` is pointed at it.

A cluster cannot see an image that exists only on your machine, so first make the image
available in the way your cluster expects:

- A local cluster usually has a command to load a local image into it. Check your local
  Kubernetes tool's documentation for how to load an image built on your machine.
- A remote cluster pulls from a registry. Push the image to a registry your nodes can read,
  then use that full image name in the manifest below.

Describe a deployment and a service in a file named `k8s.yaml`. The image is
`hello-container:local`, the local image you made available above, and
`imagePullPolicy: IfNotPresent` tells a node to skip pulling when it already has that
image. On a single-node local cluster that is all you need. On a multi-node cluster, the
image has to be present on every node that might run the pod, so load it on each one or
push it to a registry and use that image name here instead:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: hello-container
spec:
  replicas: 2
  selector:
    matchLabels:
      app: hello-container
  template:
    metadata:
      labels:
        app: hello-container
    spec:
      containers:
        - name: api
          image: hello-container:local
          imagePullPolicy: IfNotPresent
          ports:
            - containerPort: 8080
---
apiVersion: v1
kind: Service
metadata:
  name: hello-container
spec:
  selector:
    app: hello-container
  ports:
    - port: 80
      targetPort: 8080
```

Apply it and wait for the rollout to finish:

```bash
# Create the deployment and service
kubectl apply -f k8s.yaml

# Wait until both replicas are running
kubectl rollout status deployment/hello-container
# deployment "hello-container" successfully rolled out
```

Reach the service with a port-forward, then call it:

```bash
# Forward local port 8081 to the service's port 80
kubectl port-forward svc/hello-container 8081:80
```

```bash
# In another terminal, call the route through the forward
curl localhost:8081/greetings
# {"message":"Hello, Marreta!"}
```

Both replicas serve the same route. In a real cluster you would expose the service through
an Ingress or a load balancer rather than a port-forward.

## Result checkpoint

You scaffolded a project, built one image from it, and ran that same image with Docker,
Docker Compose, and Kubernetes, reaching the `/greetings` route each way.

## Next steps

- [Persist data with local services](../how-to/use-local-services.md): add a database or
  other provider, which is where Compose and Kubernetes start to earn their keep.
- [Configure environment variables](../how-to/configure-environment.md): set `MARRETA_*`
  variables and secrets per environment, including inside a container.
