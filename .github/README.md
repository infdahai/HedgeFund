# CI/CD Pipeline Setup

This directory contains GitHub Actions workflows for automated testing, building, and deployment.

## Workflows

### 1. Quick Check (`quick-check.yml`)
- **Trigger:** Every push to `main`/`develop` + All pull requests
- **Duration:** < 2 minutes
- **Jobs:**
  - Code formatting check (`cargo fmt`)
  - Clippy linting (fail on warnings)
  - Unit tests (`cargo test --lib`)
  - Build check (`cargo check`)

### 2. Full Audit (`full-audit.yml`)
- **Trigger:** Daily (2 AM UTC), Manual, Version tags, Main PRs
- **Duration:** 5-10 minutes
- **Jobs:**
  - Full test suite across Rust versions (stable + beta)
  - Integration tests (requires secrets)
  - Security audit (`cargo audit`)
  - Code coverage reporting
  - Release artifact uploads

### 3. Deploy (`deploy.yml`)
- **Trigger:** Manual approval + Version tags (`v*.*.*`)
- **Duration:** 10-15 minutes
- **Jobs:**
  - Build optimized release binary
  - Deploy to AWS Tokyo (CodeDeploy)
  - Build and push Docker image
  - Slack notifications

## Local Development

### Setup Pre-commit Hooks

```bash
cd hedgeFund
git config core.hooksPath .githooks
```

This will automatically run formatting, linting, and unit tests before each commit.

### Run Tests Locally

```bash
# Unit tests only (fast, no network)
cargo test --lib

# All tests including integration (requires .env)
cargo test --all -- --ignored

# Specific integration test
cargo test --test live_market_data -- --ignored

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --all-targets -- -D warnings
```

## Deployment Process

### Manual Deployment

1. Go to Actions tab → "Deploy" workflow
2. Click "Run workflow"
3. Select environment: `production` or `staging`
4. (Optional) Specify version tag
5. Confirm deployment

### Automated Deployment (Version Tags)

```bash
# Tag your commit with version
git tag v1.0.0
git push origin v1.0.0

# This triggers deploy workflow automatically
```

## Monitoring

After deployment:

- Check AWS CodeDeploy for deployment status
- Monitor logs: `ssh server "tail -f /home/trader/logs/opinion-hft.log"`
- Health check: `docker ps | grep opinion-hft` or `systemctl status opinion-hft`

## Troubleshooting

### Quick Check Fails

```bash
# Fix formatting
cargo fmt --all

# Fix clippy warnings
cargo clippy --all-targets --fix --allow-dirty --allow-staged
```

### Integration Tests Fail Offline

Integration tests require valid secrets in `.env`:

```bash
cd hedgeFund
cp .env.example .env
# Edit .env with actual values
cargo test --all -- --ignored
```

### Deployment Fails

1. Check CI logs in Actions tab
2. Verify AWS credentials are valid
3. Check S3 bucket exists
4. Ensure CodeDeploy application is configured

## See Also

- [SECRETS.md](../SECRETS.md) - Required GitHub secrets
- [Dockerfile](../Dockerfile) - Container image build
- [scripts/deploy.sh](../scripts/deploy.sh) - Deployment script
