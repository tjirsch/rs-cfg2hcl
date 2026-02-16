mod config;
mod schema;
mod transpiler;
mod state_migration;
mod discovery;
mod template;
mod bootstrap;

use clap::{Parser, Subcommand, CommandFactory};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use crate::schema::ResourceRegistry;
use crate::transpiler::Transpiler;
use crate::config::{Config, DiscoveryConfig};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolConfig {
    #[serde(default = "default_yaml_dir")]
    pub yaml_dir: String,
    #[serde(default = "default_hcl_dir")]
    pub hcl_dir: String,
    #[serde(default = "default_include_dirs")]
    pub include_dirs: Vec<String>,
    #[serde(default = "default_schema_dir")]
    pub schema_dir: String,
    #[serde(default = "default_tf_tool")]
    pub tf_tool: String,
    #[serde(default)]
    google_providers: Vec<String>,
    #[serde(default)]
    aws_providers: Vec<String>,
    #[serde(default)]
    azure_providers: Vec<String>,
    #[serde(default)]
    alibaba_providers: Vec<String>,
    #[serde(default = "default_version")]
    pub provider_version: String,
    #[serde(default = "default_auto_explode")]
    pub auto_explode: Vec<String>,
    #[serde(default = "default_validation_level")]
    pub validation_level: String,
    #[serde(default)]
    pub discovery_config: Option<String>,
}

impl ToolConfig {
    pub fn all_providers(&self) -> Vec<String> {
        let mut providers = Vec::new();
        providers.extend(self.google_providers.iter().map(|p| ToolConfig::parse_provider_string(p).0));
        providers.extend(self.aws_providers.iter().map(|p| ToolConfig::parse_provider_string(p).0));
        providers.extend(self.azure_providers.iter().map(|p| ToolConfig::parse_provider_string(p).0));
        providers.extend(self.alibaba_providers.iter().map(|p| ToolConfig::parse_provider_string(p).0));
        providers
    }

    pub fn parsed_providers(&self) -> Vec<(String, String)> {
        let mut providers = Vec::new();
        // default version fallback
        let def_ver = &self.provider_version;
        
        for p in &self.google_providers { providers.push(ToolConfig::parse_provider_string_with_default(p, def_ver)); }
        for p in &self.aws_providers { providers.push(ToolConfig::parse_provider_string_with_default(p, def_ver)); }
        for p in &self.azure_providers { providers.push(ToolConfig::parse_provider_string_with_default(p, def_ver)); }
        for p in &self.alibaba_providers { providers.push(ToolConfig::parse_provider_string_with_default(p, def_ver)); }
        providers
    }

    pub fn parse_provider_string(p: &str) -> (String, Option<String>) {
        if p.contains('|') {
            let parts: Vec<&str> = p.split('|').collect();
            (parts[0].trim().to_string(), Some(parts[1].trim().to_string()))
        } else {
            (p.trim().to_string(), None)
        }
    }

    pub fn parse_provider_string_with_default(p: &str, default_version: &str) -> (String, String) {
        let (name, ver) = Self::parse_provider_string(p);
        (name, ver.unwrap_or_else(|| default_version.to_string()))
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let toml_str = toml::to_string_pretty(self)?;
        fs::write(path, toml_str)
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to write config to '{}': {}", path.display(), e))) as Box<dyn std::error::Error>)?;
        Ok(())
    }
}

fn default_yaml_dir() -> String { "yaml".to_string() }
fn default_hcl_dir() -> String { "hcl".to_string() }
fn default_include_dirs() -> Vec<String> { vec!["yaml".to_string()] }
fn default_schema_dir() -> String { "schemas".to_string() }
fn default_tf_tool() -> String { "tofu".to_string() }
fn default_google_providers() -> Vec<String> { vec!["google".to_string(), "google-beta".to_string()] }
fn default_version() -> String { "7.12.0".to_string() }
fn default_auto_explode() -> Vec<String> {
    vec![
        "google_project_service".to_string(),
        ".*_iam_member".to_string(),
    ]
}
fn default_validation_level() -> String { "warn".to_string() }

mod include_processor;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to tool config file
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Validation level: warn (default), error, or none
    #[arg(long, global = true)]
    validation: Option<String>,

    /// Enable verbose output
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Transpile YAML config to HCL
    Transpile {
        /// Name of the input file (inside yaml_dir if relative)
        input: String,
        /// Name of the output file (inside hcl_dir if relative)
        #[arg(long)]
        output: Option<String>,
        /// Schema directory containing provider JSON files
        #[arg(long)]
        schema_dir: Option<PathBuf>,
    },
    /// Scan Tofu plan JSON for resource renames
    ScanPlan {
        /// Path to plan JSON file
        plan_json: PathBuf,
        /// Output mapping YAML path
        #[arg(long, default_value = "mapping.yaml")]
        output: PathBuf,
    },
    /// Generate a shell script with state mv commands from mapping
    GenerateMigration {
        /// Path to mapping YAML file
        #[arg(default_value = "mapping.yaml")]
        mapping: PathBuf,
        /// Output shell script path
        #[arg(long, default_value = "migrate.sh")]
        output: PathBuf,
    },
    /// Initialize project structure and config
    Init {
        /// Default sets to include (e.g., google)
        #[arg(long, value_delimiter = ',')]
        defaults: Option<Vec<String>>,
        /// Explicit providers to include
        #[arg(long, value_delimiter = ',')]
        providers: Option<Vec<String>>,
        #[arg(long)]
        tf_tool: Option<String>,
        /// Customer ID (workspace organization ID) to generate template for a new organization
        #[arg(long)]
        customer_id: Option<String>,
        /// Short name for the organization/customer
        #[arg(long)]
        customer_shortname: Option<String>,
        /// Billing account ID
        #[arg(long)]
        billing_account_infra: Option<String>,
        /// GCP Region
        #[arg(long)]
        default_region: Option<String>,
        /// Numeric Organization ID
        #[arg(long)]
        customer_organization_id: Option<String>,
        /// Primary Domain
        #[arg(long)]
        customer_domain: Option<String>,
        /// Infrastructure Project ID
        #[arg(long)]
        infra_project_name: Option<String>,
        /// Infrastructure Bucket Name
        #[arg(long)]
        infra_bucket_name: Option<String>,
        /// Initial IaC Admin User (default: first.admin@<domain>)
        #[arg(long)]
        iac_user: Option<String>,
    },
    /// Bootstrap initial Google Cloud infrastructure (Project, Bucket, Service Account)
    Bootstrap {
        /// The YAML config file (e.g. yaml/C01yvqxsl.yaml) to read bootstrap defaults from
        config_file: PathBuf,
        /// Dry run mode (don't create resources)
        #[arg(long)]
        dry_run: bool,
    },
    /// Fetch schemas and update config
    UpdateSchema {
        #[arg(long, value_delimiter = ',')]
        providers: Option<Vec<String>>,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        tf_tool: Option<String>,
    },
    /// Discover infrastructure and generate YAML config from Terraform state
    DiscoverFromState {
        /// Path to Terraform state JSON file
        #[arg(long)]
        state_json: Option<PathBuf>,
        /// Path to output YAML file
        #[arg(long, default_value = "discovered.yaml")]
        output: PathBuf,
        /// Add import ID to every resource
        #[arg(long)]
        add_import_id: bool,
        /// Add import ID as a comment to every resource
        #[arg(long)]
        add_import_id_as_comment: bool,
        /// Path to discovery configuration YAML file
        #[arg(long)]
        discovery_config: Option<PathBuf>,
    },
    /// Discover infrastructure and generate YAML config from GCP Organization
    DiscoverFromOrganization {
        /// Numeric Organization ID
        #[arg(long)]
        customer_organization_id: String,
        /// Path to output YAML file
        #[arg(long, default_value = "discovered.yaml")]
        output: PathBuf,
        /// Add import ID to every resource
        #[arg(long)]
        add_import_id: bool,
        /// Add import ID as a comment to every resource
        #[arg(long)]
        add_import_id_as_comment: bool,
        /// Path to discovery configuration YAML file
        #[arg(long)]
        discovery_config: Option<PathBuf>,
    },
    /// Migrate state and configuration between local and cloud modes
    Migrate {
        /// Name of the input file
        input: String,
        /// Target mode (local or cloud)
        #[arg(long)]
        mode: Option<String>,
    },
    /// Check for and install new releases from GitHub
    SelfUpdate,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cfg2hcl v{} (built {})", env!("CARGO_PKG_VERSION"), env!("BUILD_DATE"));
    let cli = Cli::parse();

    let cmd_choice = match cli.command {
        Some(c) => c,
        None => {
            if cli.verbose {
                let mut cmd = Cli::command();
                print_recursive_help(&mut cmd);
            } else {
                let mut cmd = Cli::command();
                let _ = cmd.print_help();
                println!();
            }
            std::process::exit(0);
        }
    };

    let config_file_path = if let Some(path) = &cli.config {
        path.clone()
    } else {
        let default_config = PathBuf::from("config.toml");
        if default_config.exists() {
            default_config
        } else {
            // Config is mandatory for Transpile and other commands that need it
            match cmd_choice {
                Commands::Transpile { .. } | Commands::ScanPlan { .. } | Commands::GenerateMigration { .. } | Commands::UpdateSchema { .. } | Commands::DiscoverFromState { .. } | Commands::DiscoverFromOrganization { .. } | Commands::Migrate { .. } | Commands::Bootstrap { .. } => {
                    return Err("Config file 'config.toml' not found in current directory. Please provide it or specify --config <PATH>.".into());
                }
                Commands::Init { .. } | Commands::SelfUpdate => {
                    // Init and SelfUpdate can proceed without a config file
                    PathBuf::from("config.toml")
                }
            }
        }
    };

    let config_dir = config_file_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let mut tool_config: ToolConfig = if config_file_path.exists() {
        let content = fs::read_to_string(&config_file_path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to read config file '{}': {}", config_file_path.display(), e)))?;
        toml::from_str(&content)?
    } else {
        ToolConfig {
            yaml_dir: default_yaml_dir(),
            hcl_dir: default_hcl_dir(),
            include_dirs: default_include_dirs(),
            schema_dir: default_schema_dir(),
            tf_tool: default_tf_tool(),
            google_providers: default_google_providers(),
            aws_providers: Vec::new(),
            azure_providers: Vec::new(),
            alibaba_providers: Vec::new(),
            provider_version: default_version(),
            auto_explode: default_auto_explode(),
            validation_level: default_validation_level(),
            discovery_config: None,
        }
    };

    // Create a copy for runtime use with resolved paths
    let mut runtime_config = tool_config.clone();

    // Resolve relative paths in runtime_config relative to the config file directory
    if Path::new(&runtime_config.yaml_dir).is_relative() {
        runtime_config.yaml_dir = config_dir.join(&runtime_config.yaml_dir).to_str().unwrap().to_string();
    }
    if Path::new(&runtime_config.hcl_dir).is_relative() {
        runtime_config.hcl_dir = config_dir.join(&runtime_config.hcl_dir).to_str().unwrap().to_string();
    }
    if Path::new(&runtime_config.schema_dir).is_relative() {
        runtime_config.schema_dir = config_dir.join(&runtime_config.schema_dir).to_str().unwrap().to_string();
    }
    runtime_config.include_dirs = runtime_config.include_dirs.into_iter().map(|d| {
        if Path::new(&d).is_relative() {
            config_dir.join(d).to_str().unwrap().to_string()
        } else {
            d
        }
    }).collect();


    match cmd_choice {
        Commands::Transpile { input, output, schema_dir } => {
            let validation_level = cli.validation.unwrap_or(tool_config.validation_level.clone());

            let input_path = if Path::new(&input).is_absolute() {
                PathBuf::from(&input)
            } else {
                PathBuf::from(&runtime_config.yaml_dir).join(&input)
            };

            let include_paths: Vec<PathBuf> = runtime_config.include_dirs.iter().map(PathBuf::from).collect();
            let processed_content = include_processor::process_includes(&input_path, &include_paths)?;
            let raw_value: serde_yaml::Value = serde_yaml::from_str::<serde_yaml::Value>(&processed_content).map_err(|e| {
                print_yaml_error_context(&processed_content, &e);
                e
            })?;
            let raw_value_for_vars = raw_value.clone();
            let merged_value = merge_variables(raw_value);
            let processed_value = resolve_yaml_custom_tags(merged_value);

            let config: Config = {
                serde_path_to_error::deserialize::<_, Config>(processed_value).map_err(|e: serde_path_to_error::Error<serde_yaml::Error>| {
                    let path = e.path().to_string();
                    format!("Error at '{}': {}", path, e.into_inner())
                })?
            };

            // Sync schemas based on providers in YAML
            if let Some(providers) = &config.providers {
                let provider_names: Vec<String> = providers.keys().cloned().collect();
                sync_schemas(&mut tool_config, &runtime_config, &provider_names, &config_file_path)?;
            }

            let s_dir = schema_dir.unwrap_or_else(|| PathBuf::from(&runtime_config.schema_dir));
            if !s_dir.exists() {
                fs::create_dir_all(&s_dir)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to create schema directory '{}': {}", s_dir.display(), e)))?;
            }
            let registry = ResourceRegistry::load_all(s_dir.to_str().unwrap_or("schemas"))?;

            let variables = extract_variables(&raw_value_for_vars);

            let mut provider_sources = HashMap::new();
            let mut provider_versions = HashMap::new();
            
            // Populate sources and versions from parsed config
            // Note: We need to handle source logic specifically per cloud type if possible,
            // but here we are iterating generally.
            // Let's iterate the original lists to know the "type" (google, aws, etc) or infer from name.
            
            let def_ver = tool_config.provider_version.clone();

            for p in &tool_config.google_providers {
                let (name, ver) = ToolConfig::parse_provider_string_with_default(p, &def_ver);
                let source = if name.contains('/') { name.clone() } else { format!("hashicorp/{}", name) };
                provider_sources.insert(name.clone(), source);
                provider_versions.insert(name, ver);
            }
             for p in &tool_config.aws_providers {
                let (name, ver) = ToolConfig::parse_provider_string_with_default(p, &def_ver);
                let source = if name.contains('/') { name.clone() } else { format!("hashicorp/{}", name) };
                provider_sources.insert(name.clone(), source);
                provider_versions.insert(name, ver);
             }
             for p in &tool_config.azure_providers {
                let (name, ver) = ToolConfig::parse_provider_string_with_default(p, &def_ver);
                let source = if name.contains('/') { name.clone() } else {
                     let base = if name.starts_with("azurerm") { "azurerm" } else { "azurerm" }; 
                     format!("hashicorp/{}", base)
                };
                provider_sources.insert(name.clone(), source);
                provider_versions.insert(name, ver);
             }
             for p in &tool_config.alibaba_providers {
                let (name, ver) = ToolConfig::parse_provider_string_with_default(p, &def_ver);
                provider_sources.insert(name.clone(), "aliyun/alicloud".to_string());
                provider_versions.insert(name, ver);
             }

            let transpiler = Transpiler::new(
                &config,
                Some(registry),
                runtime_config.auto_explode.clone(),
                validation_level,
                variables,
                provider_sources,
                provider_versions
            );
            let project = transpiler.transpile()?;

            // The user wants HCL files created directly in the hcl_dir
            let base_output_path = if let Some(out) = output {
                if Path::new(&out).is_absolute() {
                    PathBuf::from(out)
                } else {
                    PathBuf::from(&runtime_config.hcl_dir).join(out)
                }
            } else {
                PathBuf::from(&runtime_config.hcl_dir)
            };

            // Ensure the output directory exists
            if !base_output_path.exists() {
                fs::create_dir_all(&base_output_path)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to create output directory '{}': {}", base_output_path.display(), e)))?;
            }

            let imports_path = base_output_path.join("imports.tf");
            if imports_path.exists() {
                fs::remove_file(&imports_path)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to delete old imports.tf: {}", e)))?;
            }

            let write_file = |filename: &str, content: &str| -> std::io::Result<()> {
                if content.trim().is_empty() { return Ok(()); }
                let p = base_output_path.join(filename);
                fs::write(&p, content)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to write file '{}': {}", p.display(), e)))?;
                println!("Created {}", p.display());
                Ok(())
            };

            write_file("main.tf", &project.main_tf)?;
            write_file("providers.tf", &project.providers_tf)?;
            write_file("variables.tf", &project.variables_tf)?;
            write_file("terraform.tfvars", &project.tfvars)?;
            write_file("imports.tf", &project.imports_tf)?;
            Ok::<(), Box<dyn std::error::Error>>(())
        }
        Commands::Init {
            defaults,
            providers,
            tf_tool,
            customer_id,
            customer_shortname,
            billing_account_infra,
            default_region,
            customer_organization_id,
            customer_domain,
            infra_project_name,
            infra_bucket_name,
            iac_user,
        } => {
            let mut final_google = Vec::new();
            let mut final_aws = Vec::new();
            let mut final_azure = Vec::new();
            let mut final_alibaba = Vec::new();

            if let Some(defs) = defaults {
                for d in defs {
                    match d.as_str() {
                        "google" => {
                            final_google.extend(vec!["google".to_string(), "google-beta".to_string()]);
                        }
                        _ => {}
                    }
                }
            }

            if let Some(provs) = providers {
                // For explicit providers, we'll put them in google for now if they start with google, or general
                for p in provs {
                    if p.starts_with("google") { final_google.push(p); }
                    else if p.starts_with("aws") { final_aws.push(p); }
                    else if p.starts_with("az") { final_azure.push(p); }
                    else if p.starts_with("ali") { final_alibaba.push(p); }
                }
            }

            // Deduplicate
            final_google.sort(); final_google.dedup();

            let tool = tf_tool.unwrap_or_else(|| tool_config.tf_tool.clone());

            // 1. Create Directories
            let dirs = vec![&tool_config.yaml_dir, &tool_config.hcl_dir, &tool_config.schema_dir];
            for d in dirs {
                fs::create_dir_all(d)?;
                println!("Created directory: {}", d);
            }

            // 2. Generate config.toml if missing
            if !Path::new("config.toml").exists() {
                let mut config_lines = vec![
                    format!("schema_dir = \"{}\"", tool_config.schema_dir),
                    format!("yaml_dir = \"{}\"", tool_config.yaml_dir),
                    format!("hcl_dir = \"{}\"", tool_config.hcl_dir),
                    "include_dirs = [\".\", \"yaml\"]".to_string(),
                    format!("tf_tool = \"{}\"", tool),
                ];

                if !final_google.is_empty() {
                    config_lines.push(format!("google_providers = {:?}", final_google));
                }
                if !final_aws.is_empty() {
                    config_lines.push(format!("aws_providers = {:?}", final_aws));
                }
                if !final_azure.is_empty() {
                    config_lines.push(format!("azure_providers = {:?}", final_azure));
                }
                if !final_alibaba.is_empty() {
                    config_lines.push(format!("alibaba_providers = {:?}", final_alibaba));
                }

                config_lines.push(format!("provider_version = \"{}\"", tool_config.provider_version));
                config_lines.push(format!("auto_explode = {:?}", tool_config.auto_explode));
                config_lines.push(format!("validation_level = \"{}\"", tool_config.validation_level));

                fs::write("config.toml", config_lines.join("\n"))?;
                println!("Generated config.toml");
            }

            // 3. Generate .gitignore if missing
            if !Path::new(".gitignore").exists() {
                let gitignore_content = r#"# Terraform / OpenTofu
.terraform/
*.tfstate
*.tfstate.backup

# Tool Cache
schemas/

# OS files
.DS_Store
Thumbs.db
"#;
                fs::write(".gitignore", gitignore_content)?;
                println!("Created .gitignore");
            }

            // 4. Generate template YAML if customer_id provided
            if let Some(c_id) = customer_id {
                let yaml_path = PathBuf::from(&runtime_config.yaml_dir).join(format!("{}.yaml", c_id));
                if !yaml_path.exists() {
                    let args = crate::template::TemplateArgs {
                        customer_id: c_id.clone(),
                        shortname: customer_shortname.unwrap_or_default(),
                        billing_id: billing_account_infra.unwrap_or_default(),
                        region: default_region.unwrap_or_else(|| "europe-west3".to_string()),
                        org_id: customer_organization_id.unwrap_or_else(|| "123456789012".to_string()),
                        domain: customer_domain.clone().unwrap_or_default(),
                        project_id: infra_project_name.unwrap_or_default(),
                        bucket_id: infra_bucket_name.unwrap_or_default(),
                        iac_user: iac_user.unwrap_or_else(|| format!("first.admin@{}", customer_domain.unwrap_or_default())),
                    };
                    crate::template::generate_template(&args, &yaml_path)?;
                    println!("Generated template: {}", yaml_path.display());
                } else {
                    println!("Template already exists: {}", yaml_path.display());
                }
            }

            // 4. Fetch Schemas
            let mut all_provs = final_google;
            all_provs.extend(final_aws);
            all_provs.extend(final_azure);
            all_provs.extend(final_alibaba);

            if !all_provs.is_empty() {
                for p in all_provs {
                    println!("Fetching schema for {}...", p);
                    crate::schema::ResourceRegistry::generate_schema(
                        &tool,
                        &p,
                        &runtime_config.provider_version,
                        &format!("{}/{}.json", runtime_config.schema_dir, p)
                    )?;
                }
            }
            println!("Initialization complete.");
            Ok(())
        }
        Commands::UpdateSchema { providers, version, tf_tool } => {
            let tool = tf_tool.unwrap_or_else(|| tool_config.tf_tool.clone());
            
            // If explicit providers are given, use them with CLI version or default
            // If not, iterate all providers from config and use their specific versions
            
            if let Some(p_list) = providers {
                 let def_ver = version.unwrap_or_else(|| tool_config.provider_version.clone());
                 for prov in p_list {
                     let (p_name, p_ver) = ToolConfig::parse_provider_string_with_default(&prov, &def_ver);
                     let out = PathBuf::from(format!("{}/{}.json", runtime_config.schema_dir, p_name.split('/').last().unwrap_or(&p_name)));
                     println!("Updating schema for {} version {} using {}...", p_name, p_ver, tool);
                     ResourceRegistry::generate_schema(&tool, &p_name, &p_ver, out.to_str().unwrap())?;
                 }
            } else {
                 // Use parsed config
                 for (p_name, p_ver) in tool_config.parsed_providers() {
                      // Override if version passed (unlikely for bulk update but possible)
                      let usage_ver = version.clone().unwrap_or(p_ver);
                      let out = PathBuf::from(format!("{}/{}.json", runtime_config.schema_dir, p_name.split('/').last().unwrap_or(&p_name)));
                      println!("Updating schema for {} version {} using {}...", p_name, usage_ver, tool);
                      ResourceRegistry::generate_schema(&tool, &p_name, &usage_ver, out.to_str().unwrap())?;
                 }
            }
            println!("Done.");
            Ok(())
        }
        Commands::ScanPlan { plan_json, output } => {
            let p_json = if plan_json.is_absolute() { plan_json } else { config_dir.join(plan_json) };
            let mapping = crate::state_migration::scan_plan(&p_json)?;
            let yaml = serde_yaml::to_string(&mapping)?;

            let final_output = if output.is_absolute() { output } else { config_dir.join(output) };
            fs::write(&final_output, yaml)?;
            println!("Mapping generated: {}", final_output.display());
            Ok(())
        }
        Commands::GenerateMigration { mapping, output } => {
            let m_path = if mapping.is_absolute() { mapping } else { config_dir.join(mapping) };
            let final_output = if output.is_absolute() { output } else { config_dir.join(output) };
            crate::state_migration::generate_migration(&m_path, &final_output, &tool_config.tf_tool)?;
            println!("Migration script generated: {}", final_output.display());
            Ok(())
        }
        Commands::DiscoverFromState { state_json, output, add_import_id, add_import_id_as_comment, discovery_config } => {
            let discovery_config_obj = load_discovery_config(discovery_config, &tool_config)?
                .ok_or_else(|| {
                    let err: Box<dyn std::error::Error> = "Discovery configuration not found. Please provide --discovery-config or ensure 'presets/discovery-config.yaml' exists and is correctly configured in config.toml.".into();
                     err
                })?;
            let enabled_types = Some(discovery_config_obj.resource_types.into_iter().filter(|(_,v)| v.import).map(|(k,_)| k).collect());

            println!("Reading infrastructure state...");
            let state_val: serde_json::Value = if let Some(path) = state_json {
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read state file '{}': {}", path.display(), e))?;
                serde_json::from_str(&content)?
            } else {
                let output = std::process::Command::new(&tool_config.tf_tool)
                    .arg("show")
                    .arg("-json")
                    .output()?;
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Failed to run {} show -json: {}", tool_config.tf_tool, err).into());
                }
                serde_json::from_slice(&output.stdout)?
            };

            let s_dir = PathBuf::from(&runtime_config.schema_dir);
            let registry = ResourceRegistry::load_all(s_dir.to_str().unwrap_or("schemas")).ok();

            let discoverer = crate::discovery::Discoverer::new(state_val, registry, cli.verbose, add_import_id, add_import_id_as_comment, enabled_types);
            let config = discoverer.discover()?;

            let mut yaml = serde_yaml::to_string(&config)?;

            if add_import_id_as_comment {
                // Post-process to turn import-id-comment fields into actual YAML comments
                let mut lines: Vec<String> = Vec::new();
                for line in yaml.lines() {
                    if line.contains("import-id-comment:") {
                        let parts: Vec<&str> = line.split("import-id-comment:").collect();
                        if parts.len() == 2 {
                            let indent = parts[0];
                            let value = parts[1].trim().trim_matches('"').trim_matches('\'');
                            lines.push(format!("{}# import-id: {}", indent, value));
                            continue;
                        }
                    }
                    lines.push(line.to_string());
                }
                yaml = lines.join("\n") + "\n";
            }

            let final_output = if output.is_absolute() {
                output
            } else {
                PathBuf::from(&runtime_config.yaml_dir).join(output)
            };

            if let Some(parent) = final_output.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create output directory '{}': {}", parent.display(), e))?;
            }
            fs::write(&final_output, yaml)
                .map_err(|e| format!("Failed to write output file '{}': {}", final_output.display(), e))?;
            if cli.verbose {
                crate::discovery::Discoverer::print_summary(&config, Some(discoverer.filtered_count.get()));
            }
            Ok(())
        }
        Commands::DiscoverFromOrganization { customer_organization_id, output, add_import_id, add_import_id_as_comment, discovery_config } => {
            let s_dir = PathBuf::from(&tool_config.schema_dir);
            let registry = ResourceRegistry::load_all(s_dir.to_str().unwrap_or("schemas"))
                .map_err(|e| format!("Failed to load resource registry from {}: {}", s_dir.display(), e))?;

            let discovery_config_obj = load_discovery_config(discovery_config, &tool_config)?
                .ok_or_else(|| {
                    let err: Box<dyn std::error::Error> = "Discovery configuration not found. Please provide --discovery-config or ensure 'presets/discovery-config.yaml' exists and is correctly configured in config.toml.".into();
                     err
                })?;
            let config = crate::discovery::Discoverer::discover_from_org(&customer_organization_id, cli.verbose, add_import_id, add_import_id_as_comment, Some(discovery_config_obj), Some(registry)).await?;
            let mut yaml = serde_yaml::to_string(&config)?;

            if add_import_id_as_comment {
                // Post-process to turn import-id-comment fields into actual YAML comments
                let mut lines: Vec<String> = Vec::new();
                for line in yaml.lines() {
                    if line.contains("import-id-comment:") {
                        let parts: Vec<&str> = line.split("import-id-comment:").collect();
                        if parts.len() == 2 {
                            let indent = parts[0];
                            let value = parts[1].trim().trim_matches('"').trim_matches('\'');
                            lines.push(format!("{}# import-id: {}", indent, value));
                            continue;
                        }
                    }
                    lines.push(line.to_string());
                }
                yaml = lines.join("\n") + "\n";
            }

            let final_output = if output.is_absolute() {
                output
            } else {
                PathBuf::from(&runtime_config.yaml_dir).join(output)
            };

            if let Some(parent) = final_output.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create output directory '{}': {}", parent.display(), e))?;
            }
            fs::write(&final_output, yaml)
                 .map_err(|e| format!("Failed to write output file '{}': {}", final_output.display(), e))?;
            if cli.verbose {
                crate::discovery::Discoverer::print_summary(&config, None);
            }
            Ok(())
        }
        Commands::Bootstrap { config_file, dry_run } => {
            let config_path = if config_file.is_absolute() {
                config_file
            } else {
                PathBuf::from(&runtime_config.yaml_dir).join(config_file)
            };
            crate::bootstrap::bootstrap(
                config_path,
                dry_run,
                runtime_config,
                cli.config.clone(),
                cli.validation.clone(),
                cli.verbose,
            )
            .await?;
            Ok(())
        }
        Commands::Migrate { input, mode } => {
            let input_path = if Path::new(&input).is_absolute() {
                PathBuf::from(&input)
            } else {
                PathBuf::from(&runtime_config.yaml_dir).join(&input)
            };

            if !input_path.exists() {
                return Err(format!("Input file not found: {}", input_path.display()).into());
            }

            let content = fs::read_to_string(&input_path)
                .map_err(|e| format!("Failed to read input file '{}': {}", input_path.display(), e))?;

            // Detect current mode
            let re_cloud = regex::Regex::new(r"deployment-mode:\s+&deployment-mode\s+cloud").unwrap();
            let current_mode = if re_cloud.is_match(&content) {
                "cloud"
            } else {
                "local"
            };

            let target_mode = match mode {
                Some(m) => m,
                None => if current_mode == "local" { "cloud".to_string() } else { "local".to_string() }
            };

            if current_mode == target_mode {
                println!("Already in {} mode. No changes needed.", target_mode);
                return Ok(());
            }

            println!("Migrating from {} to {} mode...", current_mode, target_mode);

            // Update YAML while preserving formatting and anchors
            let re = regex::Regex::new(r"(?m)^\s*deployment-mode:\s+&deployment-mode\s+\w+.*$").unwrap();
            let new_content = re.replace(&content, format!("  deployment-mode: &deployment-mode {} # switch by command", target_mode)).to_string();
            fs::write(&input_path, new_content)
                .map_err(|e| format!("Failed to write updated YAML to '{}': {}", input_path.display(), e))?;
            println!("Updated YAML: {}", input_path.display());

            // Transpile
            println!("Regenerating HCL...");
            let mut cmd = std::process::Command::new(std::env::current_exe()?);
            if let Some(config_path) = &cli.config {
                cmd.arg("--config").arg(config_path);
            }
            if let Some(validation) = &cli.validation {
                cmd.arg("--validation").arg(validation);
            }
            if cli.verbose {
                cmd.arg("--verbose");
            }
            let res = cmd.arg("transpile")
                .arg(&input)
                .status()?;

            if !res.success() {
                return Err("Failed to regenerate HCL".into());
            }

            // Run Init with migrate-state
            println!("Running {} init -migrate-state...", tool_config.tf_tool);
            let res = std::process::Command::new(&tool_config.tf_tool)
                .current_dir(&runtime_config.hcl_dir)
                .arg("init")
                .arg("-migrate-state")
                .arg("-force-copy") // Automate the "yes" for state copy
                .status()?;

            if !res.success() {
                return Err(format!("Failed to migrate state using {}", tool_config.tf_tool).into());
            }

            println!("Migration to {} mode complete.", target_mode);
            Ok(())
        }
        Commands::SelfUpdate => {
            run_self_update().await
        }
    }?;

    Ok(())
}

fn extract_variables(value: &serde_yaml::Value) -> HashMap<String, serde_yaml::Value> {
    if let serde_yaml::Value::Mapping(map) = value {
        if let Some(serde_yaml::Value::Mapping(variables)) = map.get("variables") {
             let mut vars = HashMap::new();
             for (k, v) in variables {
                 if let serde_yaml::Value::String(k_str) = k {
                     vars.insert(k_str.clone(), v.clone());
                 }
             }
             return vars;
        }
    }
    HashMap::new()
}

fn merge_variables(value: serde_yaml::Value) -> serde_yaml::Value {
    if let serde_yaml::Value::Mapping(mut map) = value {
        if let Some(serde_yaml::Value::Mapping(variables)) = map.remove("variables") {
            for (k, v) in variables {
                if !map.contains_key(&k) {
                    map.insert(k, v);
                }
            }
        }
        serde_yaml::Value::Mapping(map)
    } else {
        value
    }
}

fn resolve_yaml_custom_tags(value: serde_yaml::Value) -> serde_yaml::Value {
    match value {
        serde_yaml::Value::Mapping(map) => {
            let mut new_map = serde_yaml::Mapping::new();
            for (k, v) in map {
                let processed_k = resolve_yaml_custom_tags(k);
                let key_str = processed_k.as_str().unwrap_or("").to_string();
                let mut processed_v = resolve_yaml_custom_tags(v);

                // Coerce known string fields if they are numbers
                if matches!(key_str.as_str(), "customer-organization-id" | "infra-bucket-name" | "project_id" | "org_id" | "folder_id") {
                    if let serde_yaml::Value::Number(n) = processed_v {
                        processed_v = serde_yaml::Value::String(n.to_string());
                    }
                }

                new_map.insert(processed_k, processed_v);
            }
            serde_yaml::Value::Mapping(new_map)
        }
        serde_yaml::Value::Sequence(seq) => {
            serde_yaml::Value::Sequence(seq.into_iter().map(resolve_yaml_custom_tags).collect())
        }
        serde_yaml::Value::Tagged(tagged) => {
            if tagged.tag == "!expr" {
                return serde_yaml::Value::Tagged(tagged);
            }
            if tagged.tag == "!join" {
                if let serde_yaml::Value::Sequence(items) = tagged.value {
                    let mut result = String::new();
                    for item in items {
                        let inner = resolve_yaml_custom_tags(item);
                        match inner {
                            serde_yaml::Value::String(s) => result.push_str(&s),
                            serde_yaml::Value::Number(n) => result.push_str(&n.to_string()),
                            serde_yaml::Value::Bool(b) => result.push_str(&b.to_string()),
                            _ => {}
                        }
                    }
                    return serde_yaml::Value::String(result);
                } else {
                    let inner = resolve_yaml_custom_tags(tagged.value);
                    return match inner {
                        serde_yaml::Value::String(s) => serde_yaml::Value::String(s),
                        serde_yaml::Value::Number(n) => serde_yaml::Value::String(n.to_string()),
                        _ => serde_yaml::Value::Tagged(Box::new(serde_yaml::value::TaggedValue {
                            tag: tagged.tag,
                            value: inner,
                        }))
                    };
                }
            } else if tagged.tag == "!format" {
                if let serde_yaml::Value::Sequence(items) = tagged.value {
                    if items.is_empty() { return serde_yaml::Value::Null; }
                    let fmt_v = resolve_yaml_custom_tags(items[0].clone());
                    let mut fmt = match fmt_v {
                        serde_yaml::Value::String(s) => s,
                        _ => return serde_yaml::Value::Null,
                    };
                    for i in 1..items.len() {
                        let arg = resolve_yaml_custom_tags(items[i].clone());
                        let arg_str = match arg {
                            serde_yaml::Value::String(s) => s,
                            serde_yaml::Value::Number(n) => n.to_string(),
                            serde_yaml::Value::Bool(b) => b.to_string(),
                            _ => "".to_string(),
                        };
                        fmt = fmt.replacen("{}", &arg_str, 1);
                    }
                    return serde_yaml::Value::String(fmt);
                }
            }
            serde_yaml::Value::Tagged(Box::new(serde_yaml::value::TaggedValue {
                tag: tagged.tag,
                value: resolve_yaml_custom_tags(tagged.value),
            }))
        }
        _ => value,
    }
}

fn sync_schemas(tool_config: &mut ToolConfig, runtime_config: &ToolConfig, provider_names: &[String], config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut updated = false;
    let all_known = tool_config.all_providers(); // Just names

    for p in provider_names {
        // Categorize if not already known
        let (p_name, _) = ToolConfig::parse_provider_string(p);
        
        if !all_known.contains(&p_name) {
             // Add purely as name for now, or assume default version if added dynamically
            if p_name.starts_with("google") {
                if !tool_config.google_providers.iter().any(|existing| ToolConfig::parse_provider_string(existing).0 == p_name) {
                    tool_config.google_providers.push(p.to_string());
                    updated = true;
                }
            } else if p_name.starts_with("aws") {
                if !tool_config.aws_providers.iter().any(|existing| ToolConfig::parse_provider_string(existing).0 == p_name) {
                    tool_config.aws_providers.push(p.to_string());
                    updated = true;
                }
            } else if p_name.starts_with("az") {
                if !tool_config.azure_providers.iter().any(|existing| ToolConfig::parse_provider_string(existing).0 == p_name) {
                    tool_config.azure_providers.push(p.to_string());
                    updated = true;
                }
            } else if p_name.starts_with("ali") {
                 if !tool_config.alibaba_providers.iter().any(|existing| ToolConfig::parse_provider_string(existing).0 == p_name) {
                    tool_config.alibaba_providers.push(p.to_string());
                    updated = true;
                }
            }
        }

        // Generate schema if file missing
        // For schema generation, we need the version.
        // If it's a new provider just added, it uses the global default or whatever is in the string.
        // We need to resolve the version from the tool_config (which might have been just updated)
        
        let (p_name_resolved, p_ver_resolved) = tool_config.parsed_providers().into_iter().find(|(n,_)| n == &p_name)
             .unwrap_or_else(|| ToolConfig::parse_provider_string_with_default(p, &tool_config.provider_version));

        let out_name = p_name_resolved.split('/').last().unwrap_or(&p_name_resolved);
        let schema_path = PathBuf::from(&runtime_config.schema_dir).join(format!("{}.json", out_name));
        if !schema_path.exists() {
            // Ensure schema directory exists
            fs::create_dir_all(&runtime_config.schema_dir)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to create schema directory '{}': {}", runtime_config.schema_dir, e)))?;

            println!("Generating schema for provider: {} version {}...", p_name_resolved, p_ver_resolved);
            ResourceRegistry::generate_schema(&runtime_config.tf_tool, &p_name_resolved, &p_ver_resolved, schema_path.to_str().unwrap())?;
            updated = true;
        }
    }

    if updated {
        tool_config.save(config_path)?;
        println!("Updated config.toml and schemas.");
    }

    Ok(())
}

fn load_discovery_config(path: Option<PathBuf>, tool_config: &ToolConfig) -> Result<Option<DiscoveryConfig>, Box<dyn std::error::Error>> {
    let config_path = if let Some(p) = path {
        p
    } else if let Some(p_str) = &tool_config.discovery_config {
        PathBuf::from(p_str)
    } else {
        let default = PathBuf::from("presets/discovery-config.yaml");
        if default.exists() {
            default
        } else {
            return Ok(None);
        }
    };

    if !config_path.exists() {
         return Err(format!("Discovery configuration file not found at: {}", config_path.display()).into());
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read discovery config '{}': {}", config_path.display(), e))?;
    let config: DiscoveryConfig = serde_yaml::from_str(&content)?;

    let total_types = config.resource_types.len();
    let enabled_types = config.resource_types.values().filter(|v| v.import).count();
    println!("Loaded {} resource types from discovery config file '{}' ({} enabled for import).", total_types, config_path.display(), enabled_types);

    Ok(Some(config))
}

fn print_recursive_help(cmd: &mut clap::Command) {
    let _ = cmd.print_help();
    println!("\n");

    let mut subcmds: Vec<clap::Command> = cmd.get_subcommands().cloned().collect();
    // Sort to ensure consistent output order
    subcmds.sort_by(|a, b| a.get_name().cmp(b.get_name()));

    for mut subcmd in subcmds {
        // Skip hidden commands and help subcommand to keep output clean
        if subcmd.is_hide_set() || subcmd.get_name() == "help" {
            continue;
        }
        
        println!("\n{:=<80}", "");
        println!("COMMAND: {}", subcmd.get_name());
        println!("{:=<80}\n", "");
        
        print_recursive_help(&mut subcmd);
    }
}


async fn run_self_update() -> Result<(), Box<dyn std::error::Error>> {
    const REPO: &str = "tjirsch/rs-cfg2hcl";
    const API_URL: &str = "https://api.github.com/repos";
    
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: {}", current_version);
    
    // Fetch latest release from GitHub
    let client = reqwest::Client::builder()
        .user_agent("cfg2hcl-update-checker")
        .build()?;
    
    let url = format!("{}/{}/releases/latest", API_URL, REPO);
    let response = client.get(&url).send().await?;
    
    if !response.status().is_success() {
        return Err(format!("Failed to fetch release info: {}", response.status()).into());
    }
    
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
        // assets: Vec<Asset>, // Reserved for future use (e.g., direct binary download)
    }
    
    // #[derive(Deserialize)]
    // struct Asset {
    //     name: String,
    //     browser_download_url: String,
    // }
    
    let release: Release = response.json().await?;
    
    // Extract version from tag (remove 'v' prefix if present)
    let latest_version = release.tag_name.trim_start_matches('v');
    
    println!("Latest version: {}", latest_version);
    
    // Simple version comparison (semver comparison)
    if compare_versions(current_version, latest_version) < 0 {
        println!("\n⚠️  A new version is available!");
        println!("   Current: {}", current_version);
        println!("   Latest:  {}", latest_version);
        println!("   Release: {}", release.html_url);
        println!("\n📥 Installing update...");
        
        // Find the installer script
        let installer_url = format!("https://github.com/{}/releases/latest/download/cfg2hcl-installer.sh", REPO);
        
        // Download and run installer
        let installer_script = client.get(&installer_url).send().await?.text().await?;
        
        // Write to temp file and execute
        let temp_file = std::env::temp_dir().join("cfg2hcl-installer.sh");
        std::fs::write(&temp_file, installer_script)?;
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp_file, std::fs::Permissions::from_mode(0o755))?;
            
            let status = std::process::Command::new("sh")
                .arg(&temp_file)
                .status()?;
            
            if status.success() {
                println!("✅ Update installed successfully!");
                println!("   Please restart your terminal or run: source ~/.profile");
                
                // Download and open README.md
                if let Err(e) = download_and_open_readme(&client, REPO, &latest_version).await {
                    eprintln!("⚠️  Warning: Could not download README: {}", e);
                }
            } else {
                return Err("Failed to run installer script".into());
            }
        }
        
        #[cfg(windows)]
        {
            return Err("Automatic installation on Windows is not yet supported. Please download and run the installer manually.".into());
        }
    } else {
        println!("✅ You are running the latest version!");
    }
    
    Ok(())
}

async fn download_and_open_readme(client: &reqwest::Client, repo: &str, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Get default download directory
    let download_dir = get_download_dir()?;
    let readme_path = download_dir.join(format!("cfg2hcl-{}-README.md", version));
    
    // Try to get README from the release first (in release body or assets)
    // If not available, fetch from the repo's main branch
    let readme_url = format!("https://raw.githubusercontent.com/{}/main/README.md", repo);
    
    println!("\n📄 Downloading README...");
    let readme_content = client.get(&readme_url).send().await?.text().await?;
    
    std::fs::write(&readme_path, readme_content)?;
    println!("   Saved to: {}", readme_path.display());
    
    // Open in default editor/viewer
    println!("   Opening README...");
    open_file(&readme_path)?;
    
    Ok(())
}

fn get_download_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME")?;
        Ok(PathBuf::from(home).join("Downloads"))
    }
    
    #[cfg(target_os = "linux")]
    {
        // Try XDG_DOWNLOAD_DIR first, fallback to ~/Downloads
        if let Ok(dir) = std::env::var("XDG_DOWNLOAD_DIR") {
            Ok(PathBuf::from(dir))
        } else {
            let home = std::env::var("HOME")?;
            Ok(PathBuf::from(home).join("Downloads"))
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        use std::env;
        let user_profile = env::var("USERPROFILE")?;
        Ok(PathBuf::from(user_profile).join("Downloads"))
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported platform for download directory".into())
    }
}

fn open_file(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .status()?;
    }
    
    #[cfg(target_os = "linux")]
    {
        // Try xdg-open first, fallback to $EDITOR if set
        if std::process::Command::new("xdg-open")
            .arg(path)
            .status()
            .is_err()
        {
            if let Ok(editor) = std::env::var("EDITOR") {
                std::process::Command::new(editor)
                    .arg(path)
                    .status()?;
            } else {
                return Err("Could not open file: xdg-open not available and EDITOR not set".into());
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", path.to_str().unwrap()])
            .status()?;
    }
    
    Ok(())
}

fn compare_versions(v1: &str, v2: &str) -> i32 {
    let parse_version = |v: &str| -> Vec<u32> {
        v.split('.')
            .map(|s| s.parse::<u32>().unwrap_or(0))
            .collect()
    };
    
    let v1_parts = parse_version(v1);
    let v2_parts = parse_version(v2);
    
    let max_len = v1_parts.len().max(v2_parts.len());
    
    for i in 0..max_len {
        let v1_val = v1_parts.get(i).copied().unwrap_or(0);
        let v2_val = v2_parts.get(i).copied().unwrap_or(0);
        
        if v1_val < v2_val {
            return -1;
        } else if v1_val > v2_val {
            return 1;
        }
    }
    
    0
}

fn print_yaml_error_context(content: &str, err: &serde_yaml::Error) {
    if let Some(location) = err.location() {
        let line_idx = location.line() - 1; 
        let lines: Vec<&str> = content.lines().collect();

        if line_idx < lines.len() {
             eprintln!("\nError context (line {}):", line_idx + 1);
             eprintln!("--------------------------------------------------");
             
             let start = usize::max(0, line_idx.saturating_sub(2));
             let end = usize::min(lines.len() - 1, line_idx + 2);
             
             for i in start..=end {
                 let marker = if i == line_idx { ">>" } else { "  " };
                 eprintln!("{} {:4} | {}", marker, i + 1, lines[i]);
             }
             eprintln!("--------------------------------------------------\n");
        }
    }
}
