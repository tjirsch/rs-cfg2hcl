# cfg2hcl

A tool to transpile compact YAML infrastructure definitions into OpenTofu/Terraform HCL.
Builtin functions to bootstrap a Google Cloud Organization and do state import, migration and discovery of an existing GCP Organization from state or live infrastructure.

## Folder Structure

The project is structured such that `cfg2hcl` (the tool) is kept separate from customer-specific definitions. Each customer repository follows this layout:

```text
customer-repo/ (e.g. project-root/)
├── config.toml          # Tool configuration for this customer
├── schemas/             # JSON schemas for used cloud providers
├── yaml/                # Infrastructure definitions
└── hcl/                 # Generated .tf files
```

### Global Options

These options can be placed anywhere in the command (e.g., before or after subcommands):

- `-c, --config <FILE>`: Path to tool config file. Mandatory for most commands if `config.toml` is not in the current directory.
- `-v, --validation <LEVEL>`: Validation level for mandatory parameters (`warn`, `error`, `none`).

## Installation

### Using cargo-dist Installer (Recommended)

Install the latest release using the cargo-dist installer:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/tjirsch/rs-cfg2hcl/releases/latest/download/cfg2hcl-installer.sh | sh
```

This will install `cfg2hcl` to your cargo bin directory (`~/.cargo/bin` by default).

> **Note:** The installer script is generated automatically when releases are created. If you get a 404 error, it means no releases have been published yet. Use the "From Source" method below instead.

### From Source

To install the binary to `/usr/local/bin` (requires sudo):
```bash
cargo xtask install
```
This command safely builds the release binary as your user and then uses `sudo` only for the copy step. It works from any subdirectory in the project.

Alternatively, install directly with cargo:
```bash
cargo install --path .
```
This installs to `~/.cargo/bin` (no sudo required).

## CLI Usage

### Initialize Project (`init`)
Bootstrap a new project directory with default folders, config, .gitignore, and schemas.

```bash
cfg2hcl init \
  --customer-id C01234567 \
  --customer-shortname example-org \
  --billing-account-infra A12345-B67890-C12345 \
  --customer-domain example.com \
  --customer-organization-id "123456789012"
```

**Parameters:**
- `--defaults <LIST>`: Default provider sets to include (e.g., `google`).
- `--providers <LIST>`: Explicit providers to include (e.g., `aws`, `azure`, `google`).
- `--tf-tool <TOOL>`: Terraform binary to use (default: `tofu`).
- `--customer-id <ID>`: Workspace Organization ID (e.g., `C01234567`).
- `--customer-shortname <NAME>`: Short slug for the customer.
- `--billing-account-infra <ID>`: Billing account ID.
- `--customer-organization-id <ID>`: GCP Organization ID.
- `--customer-domain <DOMAIN>`: Primary domain name.
- `--iac-user <EMAIL>`: Initial Admin User (default: `first.admin@<customer-domain>`).
- `--default-region <REGION>`: Default GCP region (default: `europe-west3`).
- `--infra-project-name <ID>`: Override for the infrastructure project ID.
- `--infra-bucket-name <NAME>`: Override for the state bucket name.

**Under the Hood:**
- Creates the standardized directory structure: `yaml/`, `hcl/`, `schemas/`.
- Generates a default `config.toml` and `.gitignore`.
- If customer details are provided, generates a template YAML file in `yaml/`.
- Fetches the latest provider schemas for the configured providers.

### Day 0 Bootstrap (`bootstrap`)
The `bootstrap` command automates the entire onboarding process for a new customer organization.

```bash
cfg2hcl bootstrap <CONFIG_FILE> [options]
```

**Parameters:**
- `<CONFIG_FILE>`: Path to the YAML config file (e.g., `yaml/C01234567.yaml`).
- `--dry-run`: Simulation mode; does not create resources.
**Tip:** Use `--dry-run` to see what resources would be created without making changes.

**Tip:** For a declarative approach, set `deployment-mode: boot` in your YAML and run `transpile`.

**Under the Hood:**
1.  **Authentication**: Uses Application Default Credentials (ADC).
2.  **Infrastructure Folder**: Checks availability or creates the top-level folder (requires `Folder Admin`).
3.  **Project Shell**: Creates the management project (project-id defaults to `shortname-iac-infra`) inside the folder.
4.  **Billing Link**: Links the project to the specified Billing Account.
5.  **Enable APIs**: Enables critical foundation APIs (Service Usage, Cloud Resource Manager, IAM, Storage).
6.  **State Bucket**: Creates the GCS bucket for Terraform state (with versioning, uniform access).
7.  **Automated Setup**:
    - **Transpile**: Converts the YAML to HCL.
    - **Init**: Runs `tofu init` to download plugins.
    - **Import**: Automatically imports the created Folder, Project, and Bucket into the local state.

### Transpile (`transpile`)
Convert your YAML configuration to production-ready HCL.

```bash
cfg2hcl transpile <INPUT> [options]
```

**Parameters:**
- `<INPUT>`: Name of the input YAML file. This is resolved relative to the `yaml_dir` defined in your config.
- `--output, -o <FILE>`: Optional output subdirectory or absolute path. By default, output goes to `hcl_dir`.
- `--schema-dir, -s <DIR>`: Override the schema directory.

**Running from subdirectories:**
You can run the transpile command from any directory (e.g., from within the `hcl/` folder) by specifying the config path. Both styles are supported:
```bash
# Global option before subcommand
cfg2hcl --config ../config.toml transpile my-infra.yaml

# Global option after subcommand (Recommended)
cfg2hcl transpile my-infra.yaml --config ../config.toml
```
This will correctly look for `../yaml/my-infra.yaml` and update the files in the current directory.

**Under the Hood:**
- Reads the YAML file and processes any `!include` tags.
- strict Validation: Checks the YAML against the loaded provider schemas `schemas/*.json` to ensure all required fields are present.
- Merges variables from the `variables` block into the configuration.
- Generates four files in the output directory:
    - `main.tf`: Resources.
    - `providers.tf`: Provider configurations and aliases.
    - `variables.tf`: Variable declarations.
    - `terraform.tfvars`: Variable values.
    - `imports.tf`: (Optional) OpenTofu `import` blocks for existing resources.

### Resource Imports

`cfg2hcl` supports declarative resource imports using the OpenTofu/Terraform 1.5+ `import` block logic. This allows you to bring existing cloud resources under management without manually running CLI `import` commands.

#### Declarative Imports (via `import-id`)

To import an existing resource, add the `import-id` tag to its definition in your YAML:

```yaml
google_org_policy_policy:
  iam.disableServiceAccountKeyCreation:
    import-id: "organizations/12345/policies/iam.disableServiceAccountKeyCreation"
    spec:
      rules:
        - enforce: "TRUE"
```

**How it works:**
- **`import-id: "<ID>"`**: Provide the full GCP resource ID.
- **`imports.tf` Generation**: The transpiler detects the `import-id` and generates a corresponding OpenTofu `import` block in `hcl/imports.tf`.
- **Automatic Lifecycle**: `imports.tf` is automatically deleted before each `transpile` run and only recreated if `import-id` tags are found.
- **Execution**: Running `tofu plan` (or `apply`) will show these resources as "to be imported".

#### Automatic Imports during Bootstrap

The `bootstrap` command automatically handles the import of core infrastructure resources (Folder, Project, and State Bucket) into your initial state so you don't have to manually link them.

> [!NOTE]
> Declarative imports require **OpenTofu** or **Terraform 1.5.0+**. For older versions, traditional CLI `tofu import` must be used.

### Mode Switching & State Migration (`migrate`)
Seamlessly move your project between development (`local`) and production (`cloud`) modes.

```bash
cfg2hcl migrate <INPUT> --mode <MODE>
```

**Parameters:**
- `<INPUT>`: Name of the input YAML file.
- `--mode, -m <MODE>`: Target mode (`local` or `cloud`).

**Under the Hood:**
- **Update YAML**: Modifies the `deployment-mode` anchor in the source YAML file.
- **Regenerate**: Runs `transpile` to update the backend configuration (Local vs GCS) and provider authentication (ADC vs Impersonation).
- **Migrate State**: Executes `tofu init -migrate-state` to safely move your terraform state to the new backend.

### Infrastructure Discovery

`cfg2hcl` provides two discovery commands to generate YAML configurations from existing infrastructure.

#### Discover from Terraform State (`discover-from-state`)
Read an existing Terraform/OpenTofu state and generate a corresponding YAML configuration.

```bash
cfg2hcl discover-from-state --output discovered.yaml
```

**Parameters:**
- `--state-json <FILE>`: Path to Terraform state JSON file (optional). If omitted, runs `tofu show -json`.
- `--output, -o <FILE>`: Path to output YAML file (default: `discovered.yaml`).
- `--add-import-id`: Add `import-id` tag to every resource for declarative imports.
- `--add-import-id-as-comment`: Add `import-id` as a comment to every resource.
- `--discovery-config <FILE>`: Path to discovery configuration YAML file (default: `presets/discovery-config.yaml`).

**Under the Hood:**
- Reads the current state (either from a file or by running `tofu show -json`).
- Reverse-engineers the resources to match the `cfg2hcl` YAML structure.
- **Configurable Filtering**: respects `presets/discovery-config.yaml` to include/exclude specific resources and attributes.
  - Resource types can be globally enabled/disabled (`import: true/false`).
  - Specific attributes can be filtered via `exclude` and `include` lists per resource.
- **Schema Validation**: Automatically validates discovered data against the Terraform Provider Schema, dropping read-only or computed fields that would cause HCL generation errors.
- **IAM Heuristics**: Intelligently maps complex IAM resources (like `google_storage_bucket_iam_member`) to simplified, project-nested YAML structures.

#### Discover from GCP Organization (`discover-from-organization`)
Discover infrastructure directly from a GCP Organization using the Cloud Asset API and generate a YAML configuration.

```bash
cfg2hcl discover-from-organization --customer-organization-id "123456789012" --output discovered.yaml
```

**Parameters:**
- `--customer-organization-id <ID>`: Numeric GCP Organization ID (required).
- `--output, -o <FILE>`: Path to output YAML file (default: `discovered.yaml`).
- `--add-import-id`: Add `import-id` tag to every resource for declarative imports.
- `--add-import-id-as-comment`: Add `import-id` as a comment to every resource.
- `--discovery-config <FILE>`: Path to discovery configuration YAML file (default: `presets/discovery-config.yaml`).

**Under the Hood:**
- Uses Google Cloud Asset API to enumerate all resources in the organization.
- Requires appropriate IAM permissions (`cloudasset.assets.searchAllResources`).
- Applies the same filtering and validation as `discover-from-state`.
- Useful for discovering infrastructure that isn't managed by Terraform/OpenTofu yet.

### Update Schemas (`update-schema`)
Refresh local provider schemas to get the latest resource definitions.

```bash
cfg2hcl update-schema --providers google,google-beta
```

**Parameters:**
- `--providers, -p <LIST>`: Comma-separated list of providers to update.
- `--version, -v <VERSION>`: Provider version to fetch (default: from config).
- `--tf-tool, -t <TOOL>`: Terraform/OpenTofu binary to use.

**Under the Hood:**
- runs `tofu init` in a temporary directory.
- runs `tofu providers schema -json` to export the latest definitions.
- Updates the JSON files in `schemas/`.

## Day 0 Onboarding Playbook

This section outlines the step-by-step process for onboarding a new Google Cloud Organization.

### Phase 1: Preparation

#### Prerequisites
Ensure the executing user has:
- **Superadmin** access to the Google Workspace / Cloud Identity.
- **Organization Administrator** role on the GCP Organization.
- **Billing Account Administrator** on the target billing account (must be granted in the Reseller Console).

#### Workspace Setup
1. Authenticate with Google Cloud:
   ```bash
   gcloud auth application-default login
   ```
2. Initialize the tool configuration and folder structure:
   ```bash
   cfg2hcl init \
     --customer-id "C01234567" \
     --customer-shortname "example-org" \
     --billing-account-infra "A12345-B67890-C12345" \
     --customer-domain "example.com" \
     --customer-organization-id "123456789012" \
     --iac-user "admin@example.com"
   ```

### Phase 2: Fundamental Infrastructure

#### 1. Bootstrap Core Resources
The `bootstrap` command automates the entire process: creating the infrastructure folder, project, bucket, linking billing, enabling foundation APIs (fixing the "chicken-and-egg" problem), and initializing the state.

```bash
cfg2hcl bootstrap yaml/C01234567.yaml
```

**What this does:**
- Creates Folder, Project, Bucket, Service Account.
- Enables Service Usage, IAM, and other core APIs.
- Assigns `Folder Admin` to the user executing the bootstrap (if missing).
- Automatically runs `transpile`, `init`, and `import` to bring resources under Terraform management.

#### 2. (Optional) Customize & Transpile
*Only needed if you modify the generated YAML configuration after bootstrap.*

Modify `yaml/C01234567.yaml` as needed, then manually generate the HCL:
```bash
cfg2hcl transpile C01234567.yaml
```

#### 3. (Optional) Configure Identity
*Only needed if the default identity setup from bootstrap was insufficient.*

If customization was done, re-run initialization:
```bash
cd hcl
tofu init
tofu apply
```



### Phase 3: Identity & Access Rollout

#### 1. Apply Management Infrastructure
Run the first Tofu apply. This creates the **Identity Groups**, attaches the necessary **IAM roles** (including `Token Creator`), and finalizes the management project.

```bash
cd hcl/
tofu plan
tofu apply
```

### Phase 4: Cloud Migration

#### 1. Perform State Migration
Toggle to `cloud` mode and move state to the GCS bucket:
```bash
cfg2hcl migrate C01234567.yaml --mode cloud
```
The tool automatically updates the YAML, switches to **Service Account Impersonation**, and runs `tofu init -migrate-state`.

#### 2. Verification
In `cloud` mode, verify that you can plan/apply using the restricted service account identity:
```bash
tofu plan
```

#### Template Variables Reference

When you run `init`, the following variables are generated in the template:

| Variable | Default | Description |
|----------|---------|-------------|
| `infra-folder-name` | `Infrastructure` | Display name for the top-level folder. Leave `""` to create the project in the root. |
| `infra-project-name` | `""` | The unique ID for the management (IaC) project. |
| `infra-bucket-name` | `""` | The name of the GCS bucket for Terraform state. |
| `customer-id` | (from CLI) | The Workspace Organization ID (e.g., `C01234567...`). |
| `customer-organization-id` | `"123456789012"` | The numeric Google Cloud Organization ID. **Note:** Always use quotes, otherwise YAML interprets this as a number. |
| `customer-domain` | `""` | The customer's primary domain (e.g., `example.com`). |
| `customer-longname` | `""` | The full legal name of the customer entity. |
| `customer-shortname` | `""` | A unique slug or shortname for the customer. |
| `svc-iac-account` | `svc-iac-001` | The name/ID of the primary IaC Service Account. |
| `svc-iac-users-group` | `svc-iac-users` | The Cloud Identity group for IaC administrators. |
| `billing-account-infra` | `""` | The Billing Account ID (e.g., `A12345-B67890-C12345`). |
| `deployment-engine` | `tofu` | The IaC tool: `tofu` or `terraform`. |
| `deployment-mode` | `local` | `local` for Day 0 (User ADC); `cloud` for Day 1+ (Impersonation). |
| `default-region` | `europe-west3` | Default region for regional resources. |
| `default-zone` | `europe-west3-a` | Default zone for zonal resources. |

### 3. Transpile
Convert a YAML file to HCL. Run this from within the customer repository directory.
```bash
cfg2hcl transpile my-infra.yaml
```
- Input is read from `yaml_dir` (e.g., `./yaml/my-infra.yaml`).
- Output is written directly to the `hcl_dir` defined in your config.
- **Run from anywhere**: All paths are resolved relative to the configuration file's directory.
- **Automatic Schema Sync**: The tool will automatically fetch missing provider schemas via `tofu/terraform` during transpilation.

## YAML Configuration

The input YAML file is the source of truth for your infrastructure.

### Terraform & Backend
The `terraform` block is mandatory and used primarily for backend configuration.

```yaml
terraform:
  backend:
    gcs:
      bucket: "my-infra-bucket"
      prefix: "project-a"
```

### Providers
Define one or more provider instances.

```yaml
providers:
  google:
    region: "europe-west3"
    zone: "europe-west3-a"
  google: # Support for multiple aliased providers
    - alias: "secondary"
      region: "us-central1"
```

### Variables
Declare variables in a `variables` block. They are automatically merged to the root context and can be used with YAML anchors.

```yaml
variables:
  customer-id: &customer-id "C34projectroot"
  region: &region "europe-west3"

google_project:
  my-project:
    project_id: *customer-id
```
- Variables are declared as `string` types in `_variables.tf`.
- Values are written to `.tfvars`.

### 3. Update Schemas
Refresh provider schemas manually.
```bash
cfg2hcl update-schema --providers google,google-beta
```

## Configuration (config.toml)

The tool reads its settings from `config.toml`. Default values are:

| Key | Default | Description |
|-----|---------|-------------|
| `yaml_dir` | `"yaml"` | Source directory for YAML files |
| `hcl_dir` | `"hcl"` | Target directory for generated HCL |
| `schema_dir` | `"schemas"` | Directory where provider schemas are cached |
| `include_dirs` | `[".", "yaml"]` | Search paths for `!include` files |
| `tf_tool` | `"tofu"` | The binary used to fetch schemas |
| `google_providers` | `["google", "google-beta"]` | List of Google providers |
| `provider_version` | `"7.12.0"` | Provider version to use |
| `auto_explode` | `["google_project_service", ".*_iam_member"]` | Resources that use compact explosion |
| `validation_level` | `"warn"` | Validation level for mandatory parameters |

## Schema Validation

The tool automatically checks your YAML against the provider schemas to ensure all mandatory parameters and blocks are present.

- **Attributes**: Checks for `required` fields (e.g., `project_id`).
- **Blocks**: Checks for mandatory blocks with `min_items > 0` (e.g., `boot_disk` for a VM).

You can control the strictness via CLI `--validation` or `config.toml`.

## YAML Features

### Custom YAML Tags
Enhance your configuration with dynamic logic:
- **`!include <file>`**: Recursively include other YAML snippets.
- **`!format [template, arg1, arg2]`**: Dynamic string formatting using placeholders (`{}`).
  ```yaml
  member: !format
    - "serviceAccount:svc-iac-001@{}.iam.gserviceaccount.com"
    - *infra-project-name
  ```
- **`!join [arg1, arg2, ...]`**: Concatenate multiple values into a single string.

### Conditional Folding
Setting a folder's `display_name` to an empty string (`""`) will skip the `google_folder` resource and "implode" its contents into the parent context. This is useful for conditionally creating folders based on variables.

### Compact Explosion (CEX)
Resources named with a `CEX_` prefix (or listed in `auto_explode`) support compact definition styles:
- **IAM**: Define many roles for one member in a simple block.
- **Services**: Enable lists of GCP services in one block.

## Core Principles

The tool follows a central design philosophy based on **Hierarchy Context**, **Attribute Inheritance**, and **Strict Validation**.

### 1. Hierarchy Context & Nesting
Resources are defined within the context of their parent in the organization hierarchy:
- **Project Context**: Resources that require a project (e.g., Buckets, VMs, Networks) are usually nested directly within a `google_project` definition.
- **Folder Context**: Resources belonging to a folder (e.g., Folder IAM members) are usually nested within a `google_folder` block.
- **Organization Context**: Organization-wide resources (e.g., Group memberships, Org IAM) are defined at the root level of the YAML.
- **Explicit Placement**: Any resource can be defined outside its logical hierarchy container if the identifying parameter (e.g., `project_id`, `folder`) is provided explicitly.

### 2. Attribute Inheritance (Narrowest Context)
Nested resources automatically inherit identity attributes from their surrounding context if not explicitly defined:
- **Automatic Matching**: The tool identifies which identifier a resource needs based on its schema (e.g., `project_id`, `project`, `folder_id`, `org_id`).
- **Inheritance**:
    - A resource inside a Project context inherits the Project ID.
    - A resource inside a Folder context inherits the Folder ID.
- **Narrowest First**: If a resource is defined in a scope where multiple contexts apply (e.g., inside a Project which is inside a Folder), it inherits from the **most specific (narrowest)** context available.
- **Explicit Override**: Explicitly provided attributes in the YAML always take precedence over inherited context values.

### 3. Context Validation & Typo Detection
To ensure configuration correctness, nested blocks are strictly validated:
- **Attribute vs. Resource**: Every key within a `Project` or `Folder` block must be either:
    - A valid native attribute/block of the parent resource (e.g., `name` for a project).
    - A valid resource type from the cloud provider schema.
- **Error Detection**: Any key that is neither a known attribute nor a known resource type is treated as a typo and triggers a **Warning**.
- **Missing Context**: Resources that require a project or folder identifier but are defined outside such a context (without an explicit identifier provided) will trigger a **Warning**.

### 4. Flexible Placement
While the tool encourages a clean hierarchy, it allows placing cross-context resources (like `google_cloud_identity_group`) inside a Project block for configuration convenience (e.g., defining project-relevant groups near the project). The transpiler will process these correctly, ignoring the project context where it doesn't apply to the resource's schema.

## Handling Resource Renames (State Migration)

If you rename a resource in your YAML, the transpiler will generate a new HCL label. OpenTofu will see this as a "delete and recreate" action. To avoid downtime, you can use the built-in migration suite:

1.  **Iterate Locally**: Use `tofu plan -out=plan.binary` and `tofu show -json plan.binary > plan.json` to identify changes.
2.  **Map Moves**: Use `cfg2hcl scan-plan plan.json` to generate a `mapping.yaml`.
3.  **Apply Renames**: Run `cfg2hcl generate-migration mapping.yaml` and execute the resulting script to perform the `mv` commands safely.

For switching between local and cloud backends, always use the high-level `cfg2hcl migrate` command.

### Scan Plan (`scan-plan`)
Analyze a Terraform/OpenTofu plan JSON file to identify resource renames and generate a mapping file.

```bash
cfg2hcl scan-plan plan.json --output mapping.yaml
```

**Parameters:**
- `<plan_json>`: Path to the plan JSON file (required).
- `--output <FILE>`: Path to output mapping YAML file (default: `mapping.yaml`).

**Under the Hood:**
- Parses the plan JSON to identify resources that are being destroyed and recreated with new addresses.
- Generates a mapping file that correlates old and new resource addresses.
- The mapping file can be used with `generate-migration` to create state move commands.

### Generate Migration (`generate-migration`)
Generate a shell script with `tofu state mv` commands from a mapping YAML file.

```bash
cfg2hcl generate-migration mapping.yaml --output migrate.sh
```

**Parameters:**
- `<mapping>`: Path to the mapping YAML file (default: `mapping.yaml`).
- `--output <FILE>`: Path to output shell script (default: `migrate.sh`).

**Under the Hood:**
- Reads the mapping file generated by `scan-plan`.
- Generates a shell script with `tofu state mv` commands to safely rename resources in the state.
- The script can be reviewed and executed manually to perform the state migration.

## Day 0: Migration Playbook

This section outlines the general process for migrating existing infrastructure into `cfg2hcl` management.

### 1. State Discovery
Begin by capturing the current infrastructure state. If you have an existing Terraform/OpenTofu project, generate a JSON state file and use the discovery tool:
```bash
tofu show -json > state.json
cfg2hcl discover-from-state --state-json state.json --output yaml/migration-discovery.yaml
```

Alternatively, if you want to discover infrastructure directly from GCP without Terraform state:
```bash
cfg2hcl discover-from-organization --customer-organization-id "123456789012" --output yaml/migration-discovery.yaml
```

### 2. Hierarchical Refinement
The discovery tool produces a relatively flat YAML structure. Organize this into the `cfg2hcl` hierarchical format:
- Move projects into their respective folders.
- Nest resources (Buckets, Networks, etc.) inside their projects to leverage **Attribute Inheritance**.
- Remove redundant attributes (like `project_id`) that are now inherited from the context.

### 3. Resource Optimization
Convert standard resource definitions into optimized `cfg2hcl` patterns:
- **Services**: Group `google_project_service` resources into a single `project_service` list.
- **IAM**: Combine individual IAM members into compact `project_iam_member` or `folder_iam_member` blocks.
- **Formatting**: Ensure attributes with sub-structures (e.g., `project_service` with `disable_on_destroy`) are correctly indented.

### 4. Validation & Reconciliation
Generate the HCL and compare it with the live environment:
1. Run `cfg2hcl transpile migration-discovery.yaml`.
2. Run `tofu plan` in the `hcl/` directory.
3. **Reconcile**: If the plan shows "replace" instead of "no changes", it means the HCL labels or resource IDs don't match.
   - Use `import-id` in the YAML to link existing resources.
   - Or use `tofu state mv` to align the existing state with the new HCL labels.

### 5. Transition to Management
Once `tofu plan` shows no changes (or only intended updates), the migration is complete. You can now manage the infrastructure exclusively through the YAML configuration.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
