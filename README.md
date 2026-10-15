# k8s-from-zero

Code for the course *Kubernetes from the CLI*: `kubelens`, a read-only health lens over
`kubectl get -o json`. It classifies pods (Ready, Pending, Done, Failing) and deployment
rollouts (Complete, InProgress, Stalled).

```sh
kubectl get pods -o json | cargo run -q -- pods
kubectl get deploy -o json | cargo run -q -- deploys
```

Exit code 1 means something is failing or stalled; 2 means bad input.

Every run of `pods` prints `contract: pod-summary-partition OK` after asserting that the
health buckets sum to the number of pods (see `contracts/`).

## Checks

```sh
cargo test
bash scripts/coverage.sh   # 100% line and function coverage by execution tracing
bash scripts/falsify.sh    # a test that calls nothing must report 0%
pv lint contracts
```

MIT licensed.
