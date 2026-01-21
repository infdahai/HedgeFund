# GitHub Secrets Configuration

This document lists all GitHub secrets required for CI/CD pipelines.

## Required Secrets (AWS Deployment)

### AWS Credentials
| Secret Name | Description | Required For |
|-------------|-------------|--------------|
| `AWS_ACCESS_KEY_ID` | AWS access key for deployment | Deploy job |
| `AWS_SECRET_ACCESS_KEY` | AWS secret key for deployment | Deploy job |
| `AWS_S3_BUCKET` | S3 bucket name for deployment artifacts | Deploy job |
| `AWS_CODEDEPLOY_APP` | CodeDeploy application name | Deploy job |
| `AWS_CODEDEPLOY_GROUP` | CodeDeploy deployment group | Deploy job |

### Docker Registry
| Secret Name | Description | Required For |
|-------------|-------------|--------------|
| `DOCKER_USERNAME` | Docker Hub username | Docker job |
| `DOCKER_PASSWORD` | Docker Hub password/token | Docker job |

### Notification
| Secret Name | Description | Required For |
|-------------|-------------|--------------|
| `SLACK_WEBHOOK` | Slack incoming webhook URL | Notify job |

## Optional Secrets (Full Audit Integration Tests)

| Secret Name | Description | Required For |
|-------------|-------------|--------------|
| `OPINION_API_KEY` | Opinion Trade API key | Integration tests |
| `OPINION_REST_URL` | Opinion Trade REST API URL | Integration tests |
| `OPINION_WS_URL` | Opinion Trade WebSocket URL | Integration tests |
| `OPINION_PRIVATE_KEY` | EOA private key (32-byte hex) | Integration tests |
| `OPINION_EOA_ADDRESS` | Externally Owned Account address | Integration tests |
| `BSC_RPC_URL` | BNB Chain RPC endpoint | Integration tests |

## How to Setup Secrets

1. Go to repository Settings → Secrets and variables → Actions
2. Click "New repository secret"
3. Add each secret listed above with appropriate values

## Running Integration Tests Locally

To run integration tests with secrets locally:

```bash
# Enable all tests (including #[ignored])
cargo test --all -- --ignored
```

Or run specific test file:

```bash
cargo test --test live_market_data -- --ignored
```

## Security Notes

- Never commit secrets to the repository
- Rotate secrets periodically (recommended: quarterly)
- Use least-privilege IAM roles for AWS access
- Read-only permissions for API keys in integration tests
