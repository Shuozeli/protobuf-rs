#!/bin/bash
# Download popular open-source .proto files for conformance testing.
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)/protos"
mkdir -p "$DIR"/{googleapis,grpc,envoy,opentelemetry,prometheus,etcd,kubernetes,buf}

BASE="https://raw.githubusercontent.com"

download() {
    local dest="$1"
    local url="$2"
    echo "  $dest"
    curl -sL "$url" -o "$DIR/$dest" --create-dirs
}

echo "=== googleapis ==="
download "googleapis/google/api/http.proto" \
    "$BASE/googleapis/googleapis/master/google/api/http.proto"
download "googleapis/google/api/annotations.proto" \
    "$BASE/googleapis/googleapis/master/google/api/annotations.proto"
download "googleapis/google/api/client.proto" \
    "$BASE/googleapis/googleapis/master/google/api/client.proto"
download "googleapis/google/api/field_behavior.proto" \
    "$BASE/googleapis/googleapis/master/google/api/field_behavior.proto"
download "googleapis/google/api/resource.proto" \
    "$BASE/googleapis/googleapis/master/google/api/resource.proto"
download "googleapis/google/rpc/status.proto" \
    "$BASE/googleapis/googleapis/master/google/rpc/status.proto"
download "googleapis/google/rpc/error_details.proto" \
    "$BASE/googleapis/googleapis/master/google/rpc/error_details.proto"
download "googleapis/google/longrunning/operations.proto" \
    "$BASE/googleapis/googleapis/master/google/longrunning/operations.proto"
download "googleapis/google/type/date.proto" \
    "$BASE/googleapis/googleapis/master/google/type/date.proto"
download "googleapis/google/type/money.proto" \
    "$BASE/googleapis/googleapis/master/google/type/money.proto"
download "googleapis/google/type/latlng.proto" \
    "$BASE/googleapis/googleapis/master/google/type/latlng.proto"

echo "=== grpc ==="
download "grpc/grpc/health/v1/health.proto" \
    "$BASE/grpc/grpc-proto/master/grpc/health/v1/health.proto"
download "grpc/grpc/reflection/v1/reflection.proto" \
    "$BASE/grpc/grpc-proto/master/grpc/reflection/v1/reflection.proto"
download "grpc/grpc/reflection/v1alpha/reflection.proto" \
    "$BASE/grpc/grpc-proto/master/grpc/reflection/v1alpha/reflection.proto"

echo "=== opentelemetry ==="
download "opentelemetry/opentelemetry/proto/common/v1/common.proto" \
    "$BASE/open-telemetry/opentelemetry-proto/main/opentelemetry/proto/common/v1/common.proto"
download "opentelemetry/opentelemetry/proto/resource/v1/resource.proto" \
    "$BASE/open-telemetry/opentelemetry-proto/main/opentelemetry/proto/resource/v1/resource.proto"
download "opentelemetry/opentelemetry/proto/trace/v1/trace.proto" \
    "$BASE/open-telemetry/opentelemetry-proto/main/opentelemetry/proto/trace/v1/trace.proto"
download "opentelemetry/opentelemetry/proto/metrics/v1/metrics.proto" \
    "$BASE/open-telemetry/opentelemetry-proto/main/opentelemetry/proto/metrics/v1/metrics.proto"
download "opentelemetry/opentelemetry/proto/logs/v1/logs.proto" \
    "$BASE/open-telemetry/opentelemetry-proto/main/opentelemetry/proto/logs/v1/logs.proto"

echo "=== prometheus ==="
download "prometheus/io/prometheus/client/metrics.proto" \
    "$BASE/prometheus/client_model/master/io/prometheus/client/metrics.proto"

echo "=== etcd ==="
download "etcd/etcd/api/etcdserverpb/rpc.proto" \
    "$BASE/etcd-io/etcd/main/api/etcdserverpb/rpc.proto"
download "etcd/etcd/api/mvccpb/kv.proto" \
    "$BASE/etcd-io/etcd/main/api/mvccpb/kv.proto"
download "etcd/etcd/api/authpb/auth.proto" \
    "$BASE/etcd-io/etcd/main/api/authpb/auth.proto"

echo "=== envoy (standalone files) ==="
download "envoy/envoy/config/core/v3/base.proto" \
    "$BASE/envoyproxy/envoy/main/api/envoy/config/core/v3/base.proto"
download "envoy/envoy/config/core/v3/address.proto" \
    "$BASE/envoyproxy/envoy/main/api/envoy/config/core/v3/address.proto"

echo "=== buf/protovalidate ==="
download "buf/buf/validate/validate.proto" \
    "$BASE/bufbuild/protovalidate/main/proto/protovalidate/buf/validate/validate.proto"

echo "=== kubernetes ==="
download "kubernetes/k8s.io/apimachinery/pkg/runtime/generated.proto" \
    "$BASE/kubernetes/apimachinery/master/pkg/runtime/generated.proto"
download "kubernetes/k8s.io/apimachinery/pkg/api/resource/generated.proto" \
    "$BASE/kubernetes/apimachinery/master/pkg/api/resource/generated.proto"
download "kubernetes/k8s.io/api/core/v1/generated.proto" \
    "$BASE/kubernetes/api/master/core/v1/generated.proto"

echo ""
echo "Downloaded $(find "$DIR" -name '*.proto' | wc -l) .proto files"
