hostname = "127.0.0.1"
port     = 8080
workers  = 8
tls      = true

allow = ["10.0.0.0/8", "192.168.0.0/16"]

headers = {
  "X-Env"  = "prod"
  "X-Team" = "platform"
}

limits {
  max_body_mb = 64
  mode        = "enforce"
}

upstream "api" {
  host = "api.internal"
  port = 8080
}

upstream "web" {
  host = "web.internal"
  port = 8081
}

rules {
  prefix   = "/api"
  upstream = "api"
}

rules {
  prefix   = "/admin"
  upstream = "web"
}
