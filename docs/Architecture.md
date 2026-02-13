# Architecture: cfg2hcl

## Overview
`cfg2hcl` is a Rust-based transpiler designed to convert compact, human-friendly YAML definitions into production-ready OpenTofu/Terraform HCL. It prioritizes structure, inheritance, and validation.

## Core Components

### 1. Transpiler (`src/transpiler.rs`)
The heart of the tool. It processes the YAML tree and generates a `main.tf`, `providers.tf`, `variables.tf`, and `terraform.tfvars`.
- **Context Awareness**: Tracks the current Organization, Folder, and Project context as it descends the YAML tree.
- **Attribute Inheritance**: Automatically injects identifiers (like `project_id`) into nested resources based on the closest parent context.
- **Conditional Folding**: Implements "implosion" logic where folders with empty display names are skipped, promoting their children to the parent context.

### 2. Schema Registry (`src/schema.rs`)
Manages Terraform provider schemas (loaded as JSON).
- **Validation**: Ensures that all required attributes and blocks are present in the YAML.
- **Translation**: Maps YAML keys to correct HCL resource types (e.g., automatically adding the `google_` prefix).

### 3. Template Generator (`src/template.rs`)
Provides a consistent starting point for new customer rollouts.
- **Declarative Bootstrap**: Generates a YAML structure representing the Day 0 infrastructure (Project, Services, Bucket, SA).

### 4. Custom YAML Processing (`src/main.rs`)
Implements custom tags to extend YAML's expressiveness:
- `!include`: Recursive file inclusion.
- `!format`: Placeholder-based string construction.
- `!join`: String concatenation.

### 5. Discovery Engine
The `discover` command reverse-engineers YAML from existing Google Cloud assets.
- **Asset Ingestion**: Consumes CAI (Cloud Asset Inventory) export streams.
- **Configurable Filtering**: Uses `discovery-config.yaml` to include/exclude resources and attribute fields.
- **Schema Validation**: Validates discovered data against Terraform schemas, automatically filtering read-only or computed fields to ensure valid HCL generation.
- **Heuristics**: intelligent mapping of IAM policies (e.g., `google_storage_bucket_iam_member`) and key generation.

## Bootstrap Workflow (Declarative Tofu)
Instead of hardcoded setup scripts, `cfg2hcl` uses a two-phase Tofu approach:
1. **Local Phase**: `deployment-mode: local`. Runs under User ADC. Creates the management project and initial Service Account.
2. **Cloud Phase**: `deployment-mode: cloud`. Uses Service Account impersonation and a GCS backend for all subsequent operations.
