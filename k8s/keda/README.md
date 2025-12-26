# KEDA Installation Guide

KEDA (Kubernetes Event Driven Autoscaler) enables autoscaling of Optimus workers based on Redis queue length.

## Prerequisites

- Kubernetes cluster running (Docker Desktop, kind, or minikube)
- kubectl configured and connected to your cluster
- Cluster admin permissions

## Installation

### Quick Install

```powershell
# Install KEDA
kubectl apply --server-side -f k8s/keda/keda-install.yaml

# Verify installation
kubectl get pods -n keda
```

Expected output:
```
NAME                                      READY   STATUS    RESTARTS   AGE
keda-operator-xxxxxxxxxx-xxxxx            1/1     Running   0          30s
keda-metrics-apiserver-xxxxxxxxxx-xxxxx   1/1     Running   0          30s
```

### Verify KEDA API

```powershell
kubectl get apiservice v1beta1.external.metrics.k8s.io
```

Should show `Available: True`

## Uninstallation

```powershell
kubectl delete -f k8s/keda/keda-install.yaml
```

## Docker Desktop Specific Notes

If using Docker Desktop with Kubernetes:

1. **Enable Kubernetes**: Docker Desktop → Settings → Kubernetes → Enable Kubernetes
2. **Context**: Ensure kubectl is using docker-desktop context
   ```powershell
   kubectl config use-context docker-desktop
   ```
3. **Image Access**: Images are shared between Docker and Kubernetes automatically

## Troubleshooting

### KEDA pods not starting

Check events:
```powershell
kubectl describe pod -n keda -l app=keda-operator
```

### APIService not available

Wait 30-60 seconds after installation, then check:
```powershell
kubectl get apiservice v1beta1.external.metrics.k8s.io -o yaml
```

### Restart KEDA

```powershell
kubectl rollout restart deployment -n keda
```

## Next Steps

After KEDA is installed:
1. Render worker manifests: `cargo run --bin optimus-cli --release -- render-k8s`
2. Deploy Optimus: `kubectl apply -f k8s/`
3. Verify ScaledObjects: `kubectl get scaledobjects -n optimus`
