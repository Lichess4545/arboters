import manifests
import arboters

manifests.yaml_stream(arboters.resources(arboters.Release {
    image_version = "0.1.0"
}))
