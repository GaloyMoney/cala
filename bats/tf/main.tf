provider "cala" {
  endpoint = "http://localhost:2252/graphql"
}

terraform {
  required_providers {
    cala = {
      source  = "registry.terraform.io/galoymoney/cala"
      version = "0.0.18"
    }
  }
}
