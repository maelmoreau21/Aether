variable "GIT_BRANCH" {
  default = "develop"
}

variable "MEDIA_VERSION" {
  default = ""
}

// Each native runner loads and validates one architecture before publication.
variable "TARGET_ARCH" {
  default = "amd64"
}

group "default" {
  targets = ["runtime", "runtime-core", "runtime-node"]
}

// One BuildKit solve shares Rust, Web and FFmpeg stages across all images.
target "common" {
  context = "."
  dockerfile = "Dockerfile"
  platforms = ["linux/${TARGET_ARCH}"]
  args = {
    GIT_BRANCH = GIT_BRANCH
    MEDIA_VERSION = MEDIA_VERSION
    CARGO_BUILD_JOBS = "2"
  }
  cache-from = ["type=gha,scope=publish-images-${TARGET_ARCH}", "type=gha,scope=ci-images-${TARGET_ARCH}"]
}

target "runtime" {
  inherits = ["common"]
  target = "runtime"
  tags = ["media-ci:runtime"]
  // Standalone includes every shared builder stage; export that cache once.
  cache-to = ["type=gha,mode=max,scope=ci-images-${TARGET_ARCH}"]
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
