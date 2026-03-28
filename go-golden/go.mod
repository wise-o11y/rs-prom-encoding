module go-golden

go 1.25.5

require github.com/prometheus/prometheus v0.0.0

require (
	github.com/cespare/xxhash/v2 v2.3.0 // indirect
	github.com/grafana/regexp v0.0.0-20250905093917-f7b3be9d1853 // indirect
	github.com/prometheus/client_model v0.6.2 // indirect
	github.com/prometheus/common v0.67.5 // indirect
	go.yaml.in/yaml/v2 v2.4.4 // indirect
	golang.org/x/text v0.35.0 // indirect
	google.golang.org/protobuf v1.36.11 // indirect
)

replace github.com/prometheus/prometheus => ../../prometheus
