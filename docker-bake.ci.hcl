variable "GIT_BRANCH" {
  default = "develop"
}

group "default" {
  targets = ["runtime", "runtime-core", "runtime-node"]
}

// One BuildKit solve shares Rust, Web and FFmpeg stages across all images.
target "common" {
  context = "."
  dockerfile = "Dockerfile"
  platforms = ["linux/amd64"]
  args = {
    GIT_BRANCH = GIT_BRANCH
    CARGO_BUILD_JOBS = "2"
  }
  cache-from = ["type=gha,scope=ci-images", "type=gha,scope=runtime", "type=gha,scope=runtime-core", "type=gha,scope=runtime-node"]
}

target "runtime" {
  inherits = ["common"]
  target = "runtime"
  tags = ["media-ci:runtime"]
  // Standalone includes every shared builder stage; export that cache once.
  cache-to = ["type=gha,mode=max,scope=ci-images"]
}

target "runtime-core" {
  inherits = ["common"]
  target = "runtime-core"
  tags = ["media-ci:runtime-core"]
}

target "runtime-node" {
  inherits = ["common"]
  target = "runtime-node"
  tags = ["media-ci:runtime-node"]
}
