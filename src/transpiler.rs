use std::collections::HashMap;
use crate::config::{Config, Folder, Project};
use crate::schema::ResourceRegistry;

pub struct GeneratedProject {
    pub main_tf: String,
    pub providers_tf: String,
    pub variables_tf: String,
    pub tfvars: String,
    pub imports_tf: String,
}

pub struct Transpiler<'a> {
    config: &'a Config,
    registry: Option<ResourceRegistry>,
    auto_explode: Vec<String>,
    validation_level: String,
    variables: HashMap<String, serde_yaml::Value>,
    provider_sources: HashMap<String, String>,
    provider_versions: HashMap<String, String>,
}

#[derive(Clone, Default)]
struct ResourceContext {
    org_id: Option<String>,
    folder_id: Option<String>,
    project_id: Option<String>,
    org_ref: Option<String>,
    folder_ref: Option<String>,
    project_ref: Option<String>,
    provider_alias: Option<String>,
}

impl<'a> Transpiler<'a> {
    pub fn new(
        config: &'a Config,
        registry: Option<ResourceRegistry>,
        auto_explode: Vec<String>,
        validation_level: String,
        variables: HashMap<String, serde_yaml::Value>,
        provider_sources: HashMap<String, String>,
        provider_versions: HashMap<String, String>,
    ) -> Self {
        Self { config, registry, auto_explode, validation_level, variables, provider_sources, provider_versions }
    }

    fn parse_hcl_expr(&self, s: &str) -> hcl::Expression {
        if s.contains('.') && !s.contains('/') && !s.contains(':') {
            let parts: Vec<&str> = s.split('.').collect();
            if let Ok(var) = hcl::Variable::new(parts[0]) {
                let mut operators = Vec::new();
                for part in &parts[1..] {
                    if let Ok(ident) = hcl::Identifier::new(*part) {
                        operators.push(hcl::TraversalOperator::GetAttr(ident));
                    } else {
                        return hcl::Expression::from(s.to_string());
                    }
                }
                return hcl::Expression::Traversal(Box::new(hcl::Traversal::new(var, operators)));
            }
        }
        hcl::Expression::from(s.to_string())
    }

    pub fn transpile(&self) -> Result<GeneratedProject, Box<dyn std::error::Error>> {
        let mut main_blocks: Vec<hcl::Block> = Vec::new();
        let mut provider_blocks: Vec<hcl::Block> = Vec::new();
        let mut variable_blocks: Vec<hcl::Block> = Vec::new();
        let mut import_blocks: Vec<hcl::Block> = Vec::new();
        let mut tfvars_lines: Vec<String> = Vec::new();

        // Terraform Block (Backend)
        // Terraform Block (Backend & Settings)
        if let Some(tf_val) = &self.config.terraform {
            let mut tf_block = hcl::Block::builder("terraform");
            let mut has_required_providers = false;

            if let serde_yaml::Value::Mapping(map) = tf_val {
                let mode = self.get_deployment_mode();
                for (k, v) in map {
                    if let serde_yaml::Value::String(k_str) = k {
                         if k_str == "backend" {
                             if let serde_yaml::Value::Mapping(be_map) = v {
                                 for (be_type, be_config) in be_map {
                                     if let serde_yaml::Value::String(be_type_str) = be_type {
                                         // Only include the backend block that matches the current mode
                                         if (mode == "local" && be_type_str == "local") || (mode == "cloud" && be_type_str == "gcs") {
                                             let mut be_builder = hcl::Block::builder("backend").add_label(be_type_str);
                                             if let serde_yaml::Value::Mapping(c_map) = be_config {
                                                 for (ck, cv) in c_map {
                                                     if let serde_yaml::Value::String(cks) = ck {
                                                         if let Some(cval) = self.yaml_to_hcl_value(cv) {
                                                             be_builder = be_builder.add_attribute((cks.as_str(), cval));
                                                         }
                                                     }
                                                 }
                                             }
                                             tf_block = tf_block.add_block(be_builder.build());
                                         }
                                     }
                                 }
                             }
                         } else if k_str == "required_providers" {
                              has_required_providers = true;
                              if let Some(rp_block) = self.yaml_to_hcl_block("required_providers", v, None) {
                                  tf_block = tf_block.add_block(rp_block);
                              }
                         } else {
                             if let Some(val) = self.yaml_to_hcl_value(v) {
                                 tf_block = tf_block.add_attribute((k_str.as_str(), val));
                             }
                         }
                    }
                }
            }

            // Add automatic required_providers if missing and we have providers
            if !has_required_providers {
                if let Some(providers) = &self.config.providers {
                    let mut rp_builder = hcl::Block::builder("required_providers");
                    for p_name in providers.keys() {
                        if let Some(source) = self.provider_sources.get(p_name) {
                            let mut p_map = hcl::Map::new();
                            p_map.insert("source".to_string(), hcl::Value::from(source.clone()));
                            if let Some(ver) = self.provider_versions.get(p_name) {
                                p_map.insert("version".to_string(), hcl::Value::from(ver.clone()));
                            }
                            rp_builder = rp_builder.add_attribute((p_name.as_str(), hcl::Value::from(p_map)));
                        }
                    }
                    tf_block = tf_block.add_block(rp_builder.build());
                }
            }
            provider_blocks.push(tf_block.build());
        } else {
            return Err("Missing 'terraform' block in YAML configuration. This is required for backend configuration.".into());
        }

        // Providers
        if let Some(providers) = &self.config.providers {
            let mut sorted_providers: Vec<_> = providers.keys().collect();
            sorted_providers.sort();

            for p_name in sorted_providers {
                let p_val = providers.get(p_name).unwrap();
                match p_val {
                    serde_yaml::Value::Sequence(seq) => {
                        for item in seq {
                            let mut builder = hcl::Block::builder("provider").add_label(p_name);
                            if let serde_yaml::Value::Mapping(map) = item {
                                let mut has_alias = false;
                                let mut project_id = None;
                                let mut has_billing_project = false;
                                let mut has_user_project_override = false;

                                for (k, v) in map {
                                    if let serde_yaml::Value::String(k_str) = k {
                                        if k_str == "alias" { has_alias = true; }
                                        if k_str == "project" { project_id = v.as_str().map(|s| s.to_string()); }
                                        if k_str == "billing_project" { has_billing_project = true; }
                                        if k_str == "user_project_override" { has_user_project_override = true; }

                                        if let Some(val) = self.yaml_to_hcl_value(v) {
                                            builder = builder.add_attribute((k_str.as_str(), val));
                                        }
                                    }
                                }
                                 if !has_alias {
                                     builder = builder.add_attribute(("alias", p_name.as_str()));
                                 }

                                 if p_name == "google" || p_name == "google-beta" {
                                     builder = self.configure_google_provider(builder, project_id, has_billing_project, has_user_project_override);
                                 }

                                 provider_blocks.push(builder.build());
                            }
                        }
                    }
                    serde_yaml::Value::Mapping(map) => {
                        let mut builder = hcl::Block::builder("provider").add_label(p_name);
                        let mut has_alias = false;
                        let mut project_id = None;
                        let mut has_billing_project = false;
                        let mut has_user_project_override = false;

                        for (k, v) in map {
                            if let serde_yaml::Value::String(k_str) = k {
                                if k_str == "alias" { has_alias = true; }
                                if k_str == "project" { project_id = v.as_str().map(|s| s.to_string()); }
                                if k_str == "billing_project" { has_billing_project = true; }
                                if k_str == "user_project_override" { has_user_project_override = true; }

                                if let Some(val) = self.yaml_to_hcl_value(v) {
                                     builder = builder.add_attribute((k_str.as_str(), val));
                                }
                            }
                        }
                        if !has_alias {
                            builder = builder.add_attribute(("alias", p_name.as_str()));
                        }

                        if p_name == "google" || p_name == "google-beta" {
                            builder = self.configure_google_provider(builder, project_id, has_billing_project, has_user_project_override);
                        }

                        provider_blocks.push(builder.build());
                    }
                    _ => {}
                }
            }
        }

        // Root Context
        let cust_org_id = self.config.extra.get("customer-organization-id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("Missing 'customer-organization-id' in configuration"));

        let root_ctx = ResourceContext {
            org_id: Some(cust_org_id.to_string()),
            org_ref: Some(format!("organizations/{}", cust_org_id)),
            provider_alias: Some("google.google".to_string()),
            ..Default::default()
        };

        // Organization Policies (google_org_policy_policy)
        if let Some(policies) = &self.config.org_policy_policy {
            let schema = if let Some(reg) = &self.registry {
                reg.find_resource("google_org_policy_policy")
                    .map(|(_, s)| s)
            } else {
                None
            };

            // org_policy_policy is modeled as HashMap<String, Value> in Config,
            // but transpile_mapping_resources expects a serde_yaml::Mapping.
            // Convert it on the fly so we can reuse the generic mapping logic.
            let mut map = serde_yaml::Mapping::new();
            for (k, v) in policies {
                map.insert(serde_yaml::Value::String(k.clone()), v.clone());
            }

            self.transpile_mapping_resources(
                &mut main_blocks,
                &mut provider_blocks,
                &mut import_blocks,
                "google_org_policy_policy",
                &map,
                schema,
                &root_ctx,
                root_ctx.provider_alias.as_deref(),
            );
        }

        // Organization IAM
        if let Some(iam_members) = &self.config.organization_iam_member {
            self.transpile_iam_members(&mut main_blocks, &mut import_blocks, iam_members, "google_organization_iam_member", "org_id", &root_ctx, root_ctx.provider_alias.as_deref(), None);
        }

        // Billing Account IAM
        if let Some(val) = &self.config.billing_account_iam_member {
            let mut members_map = HashMap::new();
            let mut explicit_id = None;

            if let serde_yaml::Value::Mapping(map) = val {
                for (k, v) in map {
                    if let serde_yaml::Value::String(k_str) = k {
                        if k_str == "billing_account_id" {
                             if let serde_yaml::Value::String(s) = v {
                                 explicit_id = Some(s.clone());
                             }
                        } else if let serde_yaml::Value::Sequence(seq) = v {
                            members_map.insert(k_str.clone(), seq.clone());
                        }
                    }
                }
            }
            // If explicit ID is not in YAML, checking variable override or falling back to assumption
            if explicit_id.is_none() {
                 if let Some(ba) = self.variables.get("billing-account-infra").and_then(|v| v.as_str()) {
                     explicit_id = Some(ba.to_string());
                 }
            }

            self.transpile_iam_members(&mut main_blocks, &mut import_blocks, &members_map, "google_billing_account_iam_member", "billing_account_id", &root_ctx, root_ctx.provider_alias.as_deref(), explicit_id);
        }

        // Folders and Projects

        // Folders and Projects
        if let Some(folders) = &self.config.folder {
            self.transpile_google_folder(&mut main_blocks, &mut provider_blocks, &mut import_blocks, folders, &root_ctx);
        }

        // Root Projects
        if let Some(projects) = &self.config.project {
            self.transpile_google_project(&mut main_blocks, &mut provider_blocks, &mut import_blocks, projects, &root_ctx);
        }

        // Root Generic Resources
        // Use google.google as default root provider to match ci.py and state
        self.transpile_generic_resources(&mut main_blocks, &mut provider_blocks, &mut import_blocks, &self.config.extra, &root_ctx, Some("google.google"));

        // Variables
        let mut sorted_vars: Vec<_> = self.variables.keys().collect();
        sorted_vars.sort();
        for key in sorted_vars {
            let val = self.variables.get(key).unwrap();

            // vars.tf: variable "key" { type = string }
            // For now, assume everything is a string or let terraform infer 'any'
            // But usually string is safe for what we see in the yaml
            variable_blocks.push(hcl::Block::builder("variable")
                .add_label(key)
                .add_attribute(("type", hcl::Expression::Variable(hcl::Variable::new("string").unwrap())))
                .build());

            // .tfvars: key = "value"
            if let Some(hcl_val) = self.yaml_to_hcl_value(val) {
                 tfvars_lines.push(format!("{} = {}", key, hcl_val.to_string()));
            }
        }

        let mut main_body = hcl::Body::builder();
        for block in main_blocks { main_body = main_body.add_block(block); }

        let mut prov_body = hcl::Body::builder();
        for block in provider_blocks { prov_body = prov_body.add_block(block); }

        let mut var_body = hcl::Body::builder();
        for block in variable_blocks { var_body = var_body.add_block(block); }

        let mut import_body = hcl::Body::builder();
        for block in import_blocks { import_body = import_body.add_block(block); }

        Ok(GeneratedProject {
            main_tf: hcl::to_string(&main_body.build())?,
            providers_tf: hcl::to_string(&prov_body.build())?,
            variables_tf: hcl::to_string(&var_body.build())?,
            tfvars: tfvars_lines.join("\n"),
            imports_tf: hcl::to_string(&import_body.build())?,
        })
    }

    fn transpile_google_folder(
        &self,
        blocks: &mut Vec<hcl::Block>,
        provider_blocks: &mut Vec<hcl::Block>,
        import_blocks: &mut Vec<hcl::Block>,
        folders: &HashMap<String, Folder>,
        ctx: &ResourceContext,
    ) {
        let mut sorted_keys: Vec<_> = folders.keys().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            let folder = folders.get(key).unwrap();
            let resource_name = key.as_str().replace("-", "_");

            // Conditional Folders: If display_name is empty, skip folder creation and promote children to current context.
            if folder.display_name.trim().is_empty() {
                if let Some(sub_folders) = &folder.folder {
                    self.transpile_google_folder(blocks, provider_blocks, import_blocks, sub_folders, ctx);
                }
                if let Some(projects) = &folder.project {
                    self.transpile_google_project(blocks, provider_blocks, import_blocks, projects, ctx);
                }
                self.transpile_generic_resources(blocks, provider_blocks, import_blocks, &folder.extra, ctx, None);
                continue;
            }

            let parent_val_expr = if let Some(pref) = &ctx.folder_ref {
                self.parse_hcl_expr(pref)
            } else {
                hcl::Expression::from(ctx.org_ref.as_ref().unwrap().clone())
            };

            let mut folder_builder = hcl::Block::builder("resource")
                .add_label("google_folder")
                .add_label(&resource_name)
                .add_attribute(("display_name", folder.display_name.clone()))
                .add_attribute(hcl::Attribute::new("parent", parent_val_expr));

            if let Some(alias) = &ctx.provider_alias {
                if let Ok(expr) = alias.parse::<hcl::Expression>() {
                    folder_builder = folder_builder.add_attribute(("provider", expr));
                }
            }

            blocks.push(folder_builder.build());

            // Generate Import Block if requested
            if let Some(id) = &folder.import_id {
                import_blocks.push(hcl::Block::builder("import")
                    .add_attribute(("to", self.parse_hcl_expr(&format!("google_folder.{}", resource_name))))
                    .add_attribute(("id", id.clone()))
                    .build());
            }

            let current_hcl_ref = format!("google_folder.{}.name", resource_name);
            let mut folder_ctx = ctx.clone();
            folder_ctx.folder_id = Some(current_hcl_ref.clone()); // Simplification: we use HCL ref as identifier in YAML usually
            folder_ctx.folder_ref = Some(current_hcl_ref);

            // Generic Resources (includes CEX_ and others in extra)
            self.transpile_generic_resources(blocks, provider_blocks, import_blocks, &folder.extra, &folder_ctx, folder_ctx.provider_alias.as_deref());

            if let Some(sub_folders) = &folder.folder {
                self.transpile_google_folder(blocks, provider_blocks, import_blocks, sub_folders, &folder_ctx);
            }
            if let Some(projects) = &folder.project {
                self.transpile_google_project(blocks, provider_blocks, import_blocks, projects, &folder_ctx);
            }
        }
    }

    fn transpile_google_project(
        &self,
        blocks: &mut Vec<hcl::Block>,
        provider_blocks: &mut Vec<hcl::Block>,
        import_blocks: &mut Vec<hcl::Block>,
        projects: &HashMap<String, Project>,
        ctx: &ResourceContext,
    ) {
        let mut sorted_keys: Vec<_> = projects.keys().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            let project = projects.get(key).unwrap();
            let resource_name = key.as_str().replace("-", "_");

            let mut block_builder = hcl::Block::builder("resource")
                .add_label("google_project")
                .add_label(&resource_name)
                .add_attribute(hcl::Attribute::new("project_id", project.project_id.clone()))
                .add_attribute(hcl::Attribute::new("name", project.name.clone().unwrap_or_else(|| project.project_id.clone())));

            if let Some(alias) = &ctx.provider_alias {
                if let Ok(expr) = alias.parse::<hcl::Expression>() {
                    block_builder = block_builder.add_attribute(("provider", expr));
                }
            }

            // Inject billing_account if missing and variable exists
            if !project.extra.contains_key("billing_account") {
                if let Some(ba) = self.variables.get("billing-account-infra") {
                    if let Some(val) = self.yaml_to_hcl_value(ba) {
                        block_builder = block_builder.add_attribute(hcl::Attribute::new("billing_account", val));
                    }
                }
            }

            let has_org = project.extra.contains_key("org_id") || project.extra.contains_key("org") || project.extra.contains_key("folder_id");
            if !has_org {
                if let Some(f_ref) = &ctx.folder_ref {
                    block_builder = block_builder.add_attribute(hcl::Attribute::new("folder_id", self.parse_hcl_expr(f_ref)));
                } else if let Some(oid) = &ctx.org_id {
                    block_builder = block_builder.add_attribute(hcl::Attribute::new("org_id", oid.clone()));
                }
            }

            // Add attributes from extra
            let (_, resource_schema) = if let Some(reg) = &self.registry {
                reg.find_resource("google_project").map(|(p, s)| (p, Some(s))).unwrap_or(("google", None))
            } else {
                ("google", None)
            };

            for (k, v) in &project.extra {
                // Filter out keys that are actually resources handled later
                let is_resource = if let Some(reg) = &self.registry {
                    reg.find_resource(k).is_some()
                } else {
                    matches!(v, serde_yaml::Value::Mapping(_)) && !matches!(k.as_str(), "labels" | "metadata" | "annotations" | "org_id" | "org" | "folder_id")
                };

                if is_resource { continue; }

                let is_block = if let Some(schema) = resource_schema {
                    schema.block.block_types.contains_key(k)
                } else {
                    matches!(v, serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_)) && !matches!(k.as_str(), "labels" | "metadata" | "annotations")
                };

                if is_block {
                    if let Some(block) = self.yaml_to_hcl_block(k, v, None) {
                        block_builder = block_builder.add_block(block);
                    }
                } else {
                    if let Some(val) = self.yaml_to_hcl_value(v) {
                        block_builder = block_builder.add_attribute(hcl::Attribute::new(k.as_str(), val));
                    }
                }
            }

            blocks.push(block_builder.build());

            // Generate Import Block if requested
            if let Some(id) = &project.import_id {
                import_blocks.push(hcl::Block::builder("import")
                    .add_attribute(("to", self.parse_hcl_expr(&format!("google_project.{}", resource_name))))
                    .add_attribute(("id", id.clone()))
                    .build());
            }

            if let Some(reg) = &self.registry {
                if let Some((_, schema)) = reg.find_resource("google_project") {
                    let mut validation_attrs = project.extra.clone();
                    validation_attrs.insert("project_id".to_string(), serde_yaml::Value::String(project.project_id.clone()));
                    if let Some(name) = &project.name {
                        validation_attrs.insert("name".to_string(), serde_yaml::Value::String(name.clone()));
                    } else {
                        validation_attrs.insert("name".to_string(), serde_yaml::Value::String(project.project_id.clone()));
                    }
                    if let Some(fid) = &ctx.folder_id {
                        validation_attrs.insert("folder_id".to_string(), serde_yaml::Value::String(fid.clone()));
                    } else if let Some(oid) = &ctx.org_id {
                        validation_attrs.insert("org_id".to_string(), serde_yaml::Value::String(oid.clone()));
                    }

                    self.validate_resource("google_project", &resource_name, &validation_attrs, schema);
                }
            }

            let project_id_ref = format!("google_project.{}.project_id", resource_name);
            let mut project_ctx = ctx.clone();
            project_ctx.project_id = Some(project.project_id.clone());
            project_ctx.project_ref = Some(project_id_ref);

            // Project specific provider for project resources
            let p_alias = format!("project_{}", key.replace("-", "_"));
            let mut p_builder = hcl::Block::builder("provider")
                .add_label("google")
                .add_attribute(("alias", p_alias.clone()))
                .add_attribute(("project", project.project_id.clone()));

            p_builder = self.configure_google_provider(p_builder, Some(project.project_id.clone()), false, false);

            // Default region if not specified (could be improved to come from project config)
            p_builder = p_builder.add_attribute(("region", "europe-west3"));

            provider_blocks.push(p_builder.build());

            let p_ref = format!("google.{}", p_alias);

            // Project Services
            if let Some(services) = &project.project_service {
                for service_val in services {
                    let project_id_ref = format!("google_project.{}.project_id", resource_name);
                    self.transpile_google_project_service(blocks, import_blocks, &project_id_ref, service_val, ctx.provider_alias.as_deref(), &resource_name);
                }
            }

            // Generic Resources (includes CEX_ and others in extra)
            self.transpile_generic_resources(blocks, provider_blocks, import_blocks, &project.extra, &project_ctx, Some(&p_ref));
        }
    }

    fn transpile_generic_resources(
        &self,
        blocks: &mut Vec<hcl::Block>,
        provider_blocks: &mut Vec<hcl::Block>,
        import_blocks: &mut Vec<hcl::Block>,
        extra: &HashMap<String, serde_yaml::Value>,
        ctx: &ResourceContext,
        provider_alias: Option<&str>,
    ) {
        let mut sorted_types: Vec<_> = extra.keys().collect();
        sorted_types.sort();

        for resource_type in sorted_types {
            let value = extra.get(resource_type).unwrap();

            // Handle CEX_ prefix for "compact" resources that need explosion
            if resource_type.starts_with("CEX_") {
                let actual_type = &resource_type[4..];
                let tf_type = if actual_type.starts_with("google_") {
                    actual_type.to_string()
                } else {
                    format!("google_{}", actual_type)
                };

                if let serde_yaml::Value::Mapping(map) = value {
                    for (key_val, items_val) in map {
                        if let (serde_yaml::Value::String(key), serde_yaml::Value::Sequence(items)) = (key_val, items_val) {
                            if tf_type.contains("iam_member") {
                                // Special case for IAM members: key is member, items are roles
                                let mut iam_map = HashMap::new();
                                iam_map.insert(key.clone(), items.clone());

                                let id_attr = if tf_type.contains("project") { "project" }
                                             else if tf_type.contains("folder") { "folder" }
                                             else if tf_type.contains("organization") { "org_id" }
                                             else { "id" };

                                if let Some(_) = ctx.project_ref.as_ref().or(ctx.folder_ref.as_ref()).or(ctx.org_ref.as_ref()) {
                                    self.transpile_iam_members(blocks, import_blocks, &iam_map, &tf_type, id_attr, ctx, provider_alias, None);
                                } else {
                                    match id_attr {
                                        "org_id" => {
                                            if let Some(_) = &ctx.org_id {
                                                self.transpile_iam_members(blocks, import_blocks, &iam_map, &tf_type, id_attr, ctx, provider_alias, None);
                                            }
                                        },
                                        _ => {}
                                    }
                                }
                            } else {
                                // TODO: Generic explosion for non-IAM resources
                            }
                        }
                    }
                }
                continue;
            }

            // Compact Cloud Identity Group Expansion
            if resource_type == "cloud_identity_group" {
                if let serde_yaml::Value::Mapping(groups) = value {
                    self.transpile_cloud_identity_groups(blocks, import_blocks, groups, provider_alias);
                }
                continue;
            }

            // Normal processing for non-prefixed or non-exploded resources
            let (tf_type, resource_schema) = if let Some(reg) = &self.registry {
                if let Some((_, schema)) = reg.find_resource(resource_type) {
                    let resolved_name = if reg.resources.contains_key(resource_type) {
                        resource_type.to_string()
                    } else if resource_type.starts_with("google_") {
                        resource_type.to_string()
                    } else {
                        format!("google_{}", resource_type)
                    };
                    (resolved_name, Some(schema))
                } else if resource_type.starts_with("google_") {
                    (resource_type.to_string(), None)
                } else {
                    (format!("google_{}", resource_type), None)
                }
            } else if resource_type.starts_with("google_") {
                (resource_type.to_string(), None)
            } else {
                (format!("google_{}", resource_type), None)
            };

            if let Some(map) = value.as_mapping() {
                self.transpile_mapping_resources(blocks, provider_blocks, import_blocks, &tf_type, map, resource_schema, ctx, provider_alias);
            } else if let Some(seq) = value.as_sequence() {
                for (i, item) in seq.iter().enumerate() {
                    if let Some(attrs) = item.as_mapping() {
                        let res_name = attrs.get(&serde_yaml::Value::String("name".to_string()))
                            .or_else(|| attrs.get(&serde_yaml::Value::String("constraint".to_string())))
                            .and_then(|v| v.as_str())
                            .map(|s| s.replace(".", "_"))
                            .unwrap_or_else(|| i.to_string());

                        self.transpile_single_resource(blocks, import_blocks, &tf_type, &res_name, attrs, resource_schema, ctx, provider_alias);
                    }
                }
            }
        }
    }

    fn transpile_mapping_resources(
        &self,
        blocks: &mut Vec<hcl::Block>,
        _provider_blocks: &mut Vec<hcl::Block>,
        import_blocks: &mut Vec<hcl::Block>,
        tf_type: &str,
        map: &serde_yaml::Mapping,
        resource_schema: Option<&crate::schema::ResourceSchema>,
        ctx: &ResourceContext,
        provider_alias: Option<&str>,
    ) {
        // Check if this tf_type is in the auto_explode list
        let mut should_explode = false;
        for pattern in &self.auto_explode {
            if self.matches_pattern(pattern, &tf_type) {
                should_explode = true;
                break;
            }
        }

        if should_explode {
            // ... (rest of explode logic)
            if let Some((_, first_val)) = map.iter().next() {
                if first_val.is_sequence() {
                    if tf_type.contains("iam_member") {
                        let mut iam_map = HashMap::new();
                        for (m_val, r_val) in map {
                            if let (serde_yaml::Value::String(m), serde_yaml::Value::Sequence(r)) = (m_val, r_val) {
                                iam_map.insert(m.clone(), r.clone());
                            }
                        }
                        let id_attr = if tf_type.contains("project") { "project" }
                                     else if tf_type.contains("folder") { "folder" }
                                     else if tf_type.contains("organization") { "org_id" }
                                     else { "id" };
                        self.transpile_iam_members(blocks, import_blocks, &iam_map, &tf_type, id_attr, ctx, provider_alias, None);
                        return;
                    } else if tf_type == "google_project_service" {
                        for (project_ref_val, s_val) in map {
                            if let (serde_yaml::Value::String(project_ref), serde_yaml::Value::Sequence(services)) = (project_ref_val, s_val) {
                                for service_val in services {
                                    let safe_project = project_ref.replace(&['.', ':'][..], "_");
                                    self.transpile_google_project_service(blocks, import_blocks, project_ref, service_val, provider_alias, &safe_project);
                                }
                            }
                        }
                        return;
                    }
                }
            }
        }

        for (res_name_val, res_attrs_val) in map {
            if let (serde_yaml::Value::String(res_name), serde_yaml::Value::Mapping(attrs)) = (res_name_val, res_attrs_val) {
                self.transpile_single_resource(blocks, import_blocks, tf_type, res_name, attrs, resource_schema, ctx, provider_alias);
            }
        }
    }

    fn transpile_single_resource(
        &self,
        blocks: &mut Vec<hcl::Block>,
        import_blocks: &mut Vec<hcl::Block>,
        tf_type: &str,
        res_name: &str,
        attrs: &serde_yaml::Mapping,
        resource_schema: Option<&crate::schema::ResourceSchema>,
        ctx: &ResourceContext,
        provider_alias: Option<&str>,
    ) {
        let label = res_name.replace("-", "_");
        let mut block_builder = hcl::Block::builder("resource").add_label(tf_type).add_label(&label);

        if let Some(alias) = provider_alias {
            if !attrs.contains_key(&serde_yaml::Value::String("provider".to_string())) {
                if let Ok(expr) = (alias).parse::<hcl::Expression>() {
                    block_builder = block_builder.add_attribute(hcl::Attribute::new("provider", expr));
                }
            }
        }

        // Inheritance and Context Logic
        let mut final_attrs = attrs.clone();

        let import_id = final_attrs.remove(&serde_yaml::Value::String("import-id".to_string()))
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        // Removal of import-existing logic (as requested by user)
        final_attrs.remove(&serde_yaml::Value::String("import-existing".to_string()));

        if tf_type == "google_project" {
            let has_org = attrs.contains_key(&serde_yaml::Value::String("org_id".to_string())) ||
                          attrs.contains_key(&serde_yaml::Value::String("org".to_string()));
            let has_folder = attrs.contains_key(&serde_yaml::Value::String("folder_id".to_string()));

            if !has_folder && !has_org {
                if let Some(f_ref) = &ctx.folder_ref {
                    block_builder = block_builder.add_attribute(hcl::Attribute::new("folder_id", self.parse_hcl_expr(f_ref)));
                    final_attrs.insert(serde_yaml::Value::String("folder_id".to_string()), serde_yaml::Value::String(f_ref.clone()));
                } else if let Some(org_id) = &ctx.org_id {
                    block_builder = block_builder.add_attribute(hcl::Attribute::new("org_id", org_id.clone()));
                    final_attrs.insert(serde_yaml::Value::String("org_id".to_string()), serde_yaml::Value::String(org_id.clone()));
                }
            }

            // Inject billing_account if missing and variable exists
            if !attrs.contains_key(&serde_yaml::Value::String("billing_account".to_string())) {
                if let Some(ba) = self.variables.get("billing-account-infra") {
                    if let Some(val) = self.yaml_to_hcl_value(ba) {
                        block_builder = block_builder.add_attribute(hcl::Attribute::new("billing_account", val));
                    }
                }
            }
        } else if tf_type == "google_org_policy_policy" {
            let name_val = attrs.get(&serde_yaml::Value::String("name".to_string()))
                .and_then(|v| v.as_str())
                .expect("Mandatory 'name' attribute missing for google_org_policy_policy");

            let has_parent = attrs.contains_key(&serde_yaml::Value::String("parent".to_string()));
            let (resolved_parent_expr, resolved_parent_str) = if has_parent {
                let v = attrs.get(&serde_yaml::Value::String("parent".to_string())).unwrap();
                (self.yaml_to_hcl_value(v), v.as_str().map(|s| s.to_string()))
            } else if let Some(p_ref) = ctx.project_ref.as_ref().or(ctx.folder_ref.as_ref()).or(ctx.org_ref.as_ref()) {
                (Some(self.parse_hcl_expr(p_ref)), Some(p_ref.clone()))
            } else {
                let org_id = ctx.org_id.as_ref().unwrap();
                (Some(hcl::Expression::from(format!("organizations/{}", org_id))), Some(format!("organizations/{}", org_id)))
            };

            // Calculate final name
            let final_name = if !name_val.contains('/') {
                match &resolved_parent_expr {
                    Some(hcl::Expression::String(p_str)) => {
                        hcl::Expression::from(format!("{}/policies/{}", p_str, name_val))
                    }
                    Some(hcl::Expression::Traversal(_)) => {
                         if let Some(p_str) = resolved_parent_str {
                             hcl::Expression::from(format!("${{{}}}/policies/{}", p_str, name_val))
                         } else {
                             hcl::Expression::from(name_val.to_owned())
                         }
                    }
                    _ => hcl::Expression::from(name_val.to_owned()),
                }
            } else {
                hcl::Expression::from(name_val.to_owned())
            };

            block_builder = block_builder.add_attribute(("name", final_name));
            if let Some(p) = &resolved_parent_expr {
                block_builder = block_builder.add_attribute(("parent", p.clone()));
            }
        } else if let Some(schema) = resource_schema {
            // Narrowest Context Inheritance
            let project_params = ["project", "project_id"];
            let folder_params = ["folder", "folder_id"];
            let org_params = ["org_id", "organization"];

            let mut context_set = false;

            // 1. Try Project Context (Narrowest)
            if let Some(p_ref) = &ctx.project_ref {
                for p in project_params {
                    if schema.block.attributes.contains_key(p) && !attrs.contains_key(&serde_yaml::Value::String(p.to_string())) {
                        block_builder = block_builder.add_attribute(hcl::Attribute::new(p, self.parse_hcl_expr(p_ref)));
                        final_attrs.insert(serde_yaml::Value::String(p.to_string()), serde_yaml::Value::String(p_ref.clone()));
                        context_set = true;
                        break;
                    }
                }
            } else if let Some(p_id) = &ctx.project_id {
                for p in project_params {
                    if schema.block.attributes.contains_key(p) && !attrs.contains_key(&serde_yaml::Value::String(p.to_string())) {
                        block_builder = block_builder.add_attribute(hcl::Attribute::new(p, p_id.clone()));
                        final_attrs.insert(serde_yaml::Value::String(p.to_string()), serde_yaml::Value::String(p_id.clone()));
                        context_set = true;
                        break;
                    }
                }
            }

            // 2. Try Folder Context
            if !context_set {
                if let Some(f_ref) = &ctx.folder_ref {
                    for f in folder_params {
                        if schema.block.attributes.contains_key(f) && !attrs.contains_key(&serde_yaml::Value::String(f.to_string())) {
                            block_builder = block_builder.add_attribute(hcl::Attribute::new(f, self.parse_hcl_expr(f_ref)));
                            final_attrs.insert(serde_yaml::Value::String(f.to_string()), serde_yaml::Value::String(f_ref.clone()));
                            context_set = true;
                            break;
                        }
                    }
                } else if let Some(f_id) = &ctx.folder_id {
                    for f in folder_params {
                        if schema.block.attributes.contains_key(f) && !attrs.contains_key(&serde_yaml::Value::String(f.to_string())) {
                            block_builder = block_builder.add_attribute(hcl::Attribute::new(f, f_id.clone()));
                            final_attrs.insert(serde_yaml::Value::String(f.to_string()), serde_yaml::Value::String(f_id.clone()));
                            context_set = true;
                            break;
                        }
                    }
                }
            }

            // 3. Try Org Context
            if !context_set {
                if let Some(o_id) = &ctx.org_id {
                    for o in org_params {
                        if schema.block.attributes.contains_key(o) && !attrs.contains_key(&serde_yaml::Value::String(o.to_string())) {
                            block_builder = block_builder.add_attribute(hcl::Attribute::new(o, o_id.clone()));
                            final_attrs.insert(serde_yaml::Value::String(o.to_string()), serde_yaml::Value::String(o_id.clone()));
                            context_set = true;
                            break;
                        }
                    }
                }
            }

            // Warning for missing required project/folder context if not set explicitly
            if !context_set {
                let needs_project = project_params.iter().any(|p| schema.block.attributes.contains_key(*p) && !attrs.contains_key(&serde_yaml::Value::String(p.to_string())));
                let needs_folder = folder_params.iter().any(|f| schema.block.attributes.contains_key(*f) && !attrs.contains_key(&serde_yaml::Value::String(f.to_string())));

                if needs_project {
                    eprintln!("Warning: Resource '{}' ({}) requires a 'project' parameter but is defined outside a project context and no explicit project is provided.", res_name, tf_type);
                } else if needs_folder {
                    eprintln!("Warning: Resource '{}' ({}) requires a 'folder' parameter but is defined outside a folder context and no explicit folder is provided.", res_name, tf_type);
                }
            }
        }

        for (k, v) in &final_attrs {
            if let serde_yaml::Value::String(k_str) = k {
                if (tf_type == "google_org_policy_policy" && (k_str == "name" || k_str == "constraint" || k_str == "parent")) ||
                   ["project", "project_id", "folder", "folder_id", "org_id", "organization", "import-id", "import-existing"].contains(&k_str.as_str()) {
                    continue;
                }

                // Special handling for parameterized constraints in google_org_policy_policy
                // Supports both `spec` and `dry_run_spec` blocks with identical structure.
                if tf_type == "google_org_policy_policy" && (k_str == "spec" || k_str == "dry_run_spec") {
                    if let serde_yaml::Value::Mapping(spec_map) = v {
                        if let Some(serde_yaml::Value::Sequence(rules_seq)) = spec_map.get(&serde_yaml::Value::String("rules".to_string())) {
                            let mut spec_builder = hcl::Block::builder(k_str.as_str());

                            // Copy other spec fields
                            for (sk, sv) in spec_map {
                                if let serde_yaml::Value::String(sks) = sk {
                                    if sks != "rules" {
                                        if let Some(val) = self.yaml_to_hcl_value(sv) {
                                            spec_builder = spec_builder.add_attribute((sks.as_str(), val));
                                        }
                                    }
                                }
                            }

                            for rule in rules_seq {
                                if let serde_yaml::Value::Mapping(rule_map) = rule {
                                    let mut rule_builder = hcl::Block::builder("rules");
                                    for (rk, rv) in rule_map {
                                        if let serde_yaml::Value::String(rks) = rk {
                                            if rks == "parameters" {
                                                // Parameters must be a JSON string. If the user provided
                                                // a structured YAML value, JSON-encode it. If it's already
                                                // a string, pass it through unchanged.
                                                match rv {
                                                    serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_) => {
                                                        if let Ok(json_str) = serde_json::to_string(rv) {
                                                            rule_builder = rule_builder.add_attribute((
                                                                "parameters",
                                                                hcl::Value::from(json_str),
                                                            ));
                                                        }
                                                    }
                                                    _ => {
                                                        if let Some(val) = self.yaml_to_hcl_value(rv) {
                                                            rule_builder = rule_builder.add_attribute((
                                                                "parameters",
                                                                val,
                                                            ));
                                                        }
                                                    }
                                                }
                                            } else if rks == "values" {
                                                // `values` is a nested block whose fields (like
                                                // `allowed_values`, `denied_values`) must be
                                                // attributes, not nested blocks.
                                                if let serde_yaml::Value::Mapping(vmap) = rv {
                                                    let mut values_builder = hcl::Block::builder("values");
                                                    for (vk, vv) in vmap {
                                                        if let serde_yaml::Value::String(vks) = vk {
                                                            if let Some(val) = self.yaml_to_hcl_value(vv) {
                                                                values_builder = values_builder.add_attribute((
                                                                    vks.as_str(),
                                                                    val,
                                                                ));
                                                            }
                                                        }
                                                    }
                                                    rule_builder = rule_builder.add_block(values_builder.build());
                                                }
                                            } else if rks == "condition" {
                                                // `condition` remains a nested block with simple attributes.
                                                if let Some(blk) = self.yaml_to_hcl_block(&rks, rv, None) {
                                                    rule_builder = rule_builder.add_block(blk);
                                                }
                                            } else if let Some(val) = self.yaml_to_hcl_value(rv) {
                                                // Simple attributes like "enforce"
                                                rule_builder = rule_builder.add_attribute((rks.as_str(), val));
                                            } else if let Some(blk) = self.yaml_to_hcl_block(&rks, rv, None) {
                                                rule_builder = rule_builder.add_block(blk);
                                            }
                                        }
                                    }
                                    spec_builder = spec_builder.add_block(rule_builder.build());
                                }
                            }
                            block_builder = block_builder.add_block(spec_builder.build());
                            continue; // Skip standard processing for spec
                        }
                    }
                }

                let is_block = if let Some(schema) = resource_schema {
                    schema.block.block_types.contains_key(k_str)
                } else {
                    matches!(v, serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_)) && !matches!(k_str.as_str(), "labels" | "metadata" | "annotations")
                };

                if is_block {
                    let nested_schema = resource_schema.and_then(|s| s.block.block_types.get(k_str).map(|bts| &bts.block));
                    if let serde_yaml::Value::Sequence(seq) = v {
                        for item in seq {
                            if let Some(block) = self.yaml_to_hcl_block(k_str, item, nested_schema) {
                                block_builder = block_builder.add_block(block);
                            }
                        }
                    } else if let Some(block) = self.yaml_to_hcl_block(k_str, v, nested_schema) {
                        block_builder = block_builder.add_block(block);
                    }
                } else {
                    if let Some(val) = self.yaml_to_hcl_value(v) {
                        block_builder = block_builder.add_attribute(hcl::Attribute::new(k_str.as_str(), val));
                    }
                }
            }
        }

        if let Some(schema) = resource_schema {
            let mut val_attrs = HashMap::new();
            for (k, v) in final_attrs {
                if let serde_yaml::Value::String(ks) = k {
                    val_attrs.insert(ks, v);
                }
            }
            self.validate_resource(tf_type, res_name, &val_attrs, schema);
        }

        blocks.push(block_builder.build());

        // Generate Import Block if requested
        if let Some(id) = import_id {
            import_blocks.push(hcl::Block::builder("import")
                .add_attribute(("to", self.parse_hcl_expr(&format!("{}.{}", tf_type, label))))
                .add_attribute(("id", id))
                .build());
        }
    }

    fn transpile_iam_members(
        &self,
        blocks: &mut Vec<hcl::Block>,
        import_blocks: &mut Vec<hcl::Block>,
        iam_members: &HashMap<String, Vec<serde_yaml::Value>>,
        resource_type: &str,
        id_attribute: &str,
        ctx: &ResourceContext,
        provider_alias: Option<&str>,
        explicit_parent_id: Option<String>,
    ) {
        let parent_expr_str_option = match id_attribute {
            "project" | "project_id" => ctx.project_ref.as_deref().or(ctx.project_id.as_deref()),
            "folder" | "folder_id" => ctx.folder_ref.as_deref().or(ctx.folder_id.as_deref()),
            "org_id" => ctx.org_id.as_deref().or(ctx.org_ref.as_deref()),
            _ => None,
        };

        let parent_val_expr = if let Some(explicit) = explicit_parent_id {
            self.parse_hcl_expr(&explicit)
        } else {
            self.parse_hcl_expr(parent_expr_str_option.unwrap_or(""))
        };

        for (member, roles) in iam_members {
            for role_val in roles {
                let (role, condition_val, import_id) = match role_val {
                    serde_yaml::Value::String(s) => (s.clone(), None, None),
                    serde_yaml::Value::Mapping(m) => {
                        let mut role = String::new();
                        let mut condition_val = None;
                        let mut import_id = None;
                        for (k, v) in m {
                            if let serde_yaml::Value::String(k_str) = k {
                                if k_str == "condition" {
                                    condition_val = Some(v);
                                } else if k_str == "import-id" {
                                    import_id = v.as_str().map(|s| s.to_string());
                                } else {
                                    role = k_str.clone();
                                }
                            }
                        }
                        if role.is_empty() {
                            continue;
                        }
                        (role, condition_val, import_id)
                    }
                    _ => {
                        eprintln!("DEBUG: Role value is not string or mapping: {:?}", role_val);
                        continue;
                    }
                };

                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                member.hash(&mut hasher);
                role.hash(&mut hasher);
                if let Some(cv) = condition_val {
                    // Simple hash of the condition value
                    format!("{:?}", cv).hash(&mut hasher);
                }
                let label = format!("iam_{}_{:x}", member.replace(&['@', '.', ':', '-'][..], "_"), hasher.finish());

                let mut rb = hcl::Block::builder("resource")
                    .add_label(resource_type)
                    .add_label(&label)
                    .add_attribute(("role", role))
                    .add_attribute(("member", member.clone()))
                    .add_attribute((id_attribute, parent_val_expr.clone()));

                if let Some(cv) = condition_val {
                    if let Some(cond_block) = self.yaml_to_hcl_block("condition", cv, None) {
                        rb = rb.add_block(cond_block);
                    }
                }

                if let Some(alias) = provider_alias {
                    if let Ok(expr) = (alias).parse::<hcl::Expression>() {
                        rb = rb.add_attribute(("provider", expr));
                    }
                }

                blocks.push(rb.build());

                // Generate Import Block if requested
                if let Some(id) = import_id {
                    import_blocks.push(hcl::Block::builder("import")
                        .add_attribute(("to", self.parse_hcl_expr(&format!("{}.{}", resource_type, label))))
                        .add_attribute(("id", id))
                        .build());
                }
            }
        }
    }

    fn validate_resource(&self, tf_type: &str, name: &str, attrs: &HashMap<String, serde_yaml::Value>, schema: &crate::schema::ResourceSchema) {
        if self.validation_level == "none" { return; }

        for (attr_name, attr_schema) in &schema.block.attributes {
            if attr_schema.required && !attrs.contains_key(attr_name) {
                // Special case for project/project_id which might be injected
                if (attr_name == "project" || attr_name == "project_id") && (attrs.contains_key("project") || attrs.contains_key("project_id")) {
                    continue;
                }

                let msg = format!("Missing mandatory parameter '{}' for resource '{}' ({})", attr_name, name, tf_type);
                if self.validation_level == "error" {
                    eprintln!("Error: {}", msg);
                    std::process::exit(1);
                } else {
                    eprintln!("Warning: {}", msg);
                }
            }
        }

        for (block_name, block_schema) in &schema.block.block_types {
            if let Some(min) = block_schema.min_items {
                if min > 0 && !attrs.contains_key(block_name) {
                    let msg = format!("Missing mandatory block '{}' for resource '{}' ({})", block_name, name, tf_type);
                    if self.validation_level == "error" {
                        eprintln!("Error: {}", msg);
                        std::process::exit(1);
                    } else {
                        eprintln!("Warning: {}", msg);
                    }
                }
            }
        }

        // Check for unknown fields
        for attr_name in attrs.keys() {
            // Special cases for meta-arguments and handled fields
            if attr_name == "depends_on" || attr_name == "lifecycle" || attr_name == "provider" || attr_name == "count" || attr_name == "for_each" {
                 continue;
            }
            if tf_type == "google_org_policy_policy" && (attr_name == "constraint" || attr_name == "type") {
                 continue;
            }
            if tf_type == "google_project" && (attr_name == "storage_bucket" || attr_name == "service_account" || attr_name == "project_iam_member" || attr_name == "project_service" || attr_name == "bigquery_dataset") {
                 continue;
            }

            // Automatically ignore if it's a resource type (nested resource)
            if let Some(reg) = &self.registry {
                if reg.find_resource(attr_name).is_some() {
                    continue;
                }
            }

            let is_known_attr = schema.block.attributes.contains_key(attr_name);
            let is_known_block = schema.block.block_types.contains_key(attr_name);

            if !is_known_attr && !is_known_block {
                // If not known, check if it's a parentage hint (project/project_id) which we allow even if not in schema
                if attr_name == "project" || attr_name == "project_id" {
                    continue;
                }

                let msg = format!("Unknown field '{}' for resource '{}' ({})", attr_name, name, tf_type);
                if self.validation_level == "error" {
                    eprintln!("Error: {}", msg);
                    std::process::exit(1);
                } else {
                    eprintln!("Warning: {}", msg);
                }
            }
        }
    }

    fn yaml_to_hcl_value(&self, v: &serde_yaml::Value) -> Option<hcl::Expression> {
        match v {
            serde_yaml::Value::Tagged(tagged) if tagged.tag == "!expr" => {
                if let serde_yaml::Value::String(s) = &tagged.value {
                    s.parse::<hcl::Expression>().ok()
                } else {
                    None
                }
            }
            serde_yaml::Value::String(s) => Some(hcl::Expression::from(s.clone())),
            serde_yaml::Value::Bool(b) => Some(hcl::Expression::from(*b)),
            serde_yaml::Value::Number(n) => {
                if n.is_i64() { Some(hcl::Expression::from(n.as_i64().unwrap())) }
                else if n.is_f64() { Some(hcl::Expression::from(n.as_f64().unwrap())) }
                else { None }
            }
            serde_yaml::Value::Sequence(seq) => {
                let exprs: Vec<hcl::Expression> = seq.iter().filter_map(|v| self.yaml_to_hcl_value(v)).collect();
                Some(hcl::Expression::Array(exprs))
            }
            serde_yaml::Value::Mapping(map) => {
                let mut hcl_obj = hcl::Object::new();
                for (mk, mv) in map {
                    if let serde_yaml::Value::String(mks) = mk {
                        if let Some(mve) = self.yaml_to_hcl_value(mv) {
                            hcl_obj.insert(hcl::ObjectKey::from(mks.clone()), mve);
                        }
                    }
                }
                Some(hcl::Expression::Object(hcl_obj))
            }
            _ => None,
        }
    }

    fn yaml_to_hcl_block(&self, name: &str, v: &serde_yaml::Value, schema: Option<&crate::schema::BlockSchema>) -> Option<hcl::Block> {
        if let serde_yaml::Value::Mapping(map) = v {
            let mut builder = hcl::Block::builder(name);
            for (bk, bv) in map {
                if let serde_yaml::Value::String(bks) = bk {
                    let is_nested_block = if let Some(s) = schema {
                        s.block_types.contains_key(bks)
                    } else {
                        // Heuristic if no schema
                        matches!(bv, serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_)) && !matches!(bks.as_str(), "labels" | "metadata" | "annotations")
                    };

                    if is_nested_block {
                        let nested_schema = schema.and_then(|s| s.block_types.get(bks).map(|bts| &bts.block));
                        if let serde_yaml::Value::Sequence(seq) = bv {
                            for item in seq {
                                if let Some(nb) = self.yaml_to_hcl_block(bks, item, nested_schema) {
                                    builder = builder.add_block(nb);
                                }
                            }
                        } else if let Some(nb) = self.yaml_to_hcl_block(bks, bv, nested_schema) {
                            builder = builder.add_block(nb);
                        }
                    } else {
                        if let Some(val) = self.yaml_to_hcl_value(bv) {
                            builder = builder.add_attribute((bks.as_str(), val));
                        }
                    }
                }
            }
            Some(builder.build())
        } else { None }
    }

    fn transpile_cloud_identity_groups(&self, blocks: &mut Vec<hcl::Block>, import_blocks: &mut Vec<hcl::Block>, groups: &serde_yaml::Mapping, provider_alias: Option<&str>) {
        let customer_id = self.config.extra.get("customer-id").and_then(|v| v.as_str()).unwrap_or("");
        let customer_domain = self.config.extra.get("customer-domain").and_then(|v| v.as_str()).unwrap_or("");

        for (g_name_val, g_attrs_val) in groups {
            if let (serde_yaml::Value::String(group_name), serde_yaml::Value::Mapping(attrs)) = (g_name_val, g_attrs_val) {
                let resource_name = group_name.replace("-", "_");

                let mut builder = hcl::Block::builder("resource")
                    .add_label("google_cloud_identity_group")
                    .add_label(&resource_name);

            if let Some(alias) = provider_alias {
                 if let Ok(expr) = (alias).parse::<hcl::Expression>() {
                     builder = builder.add_attribute(("provider", expr));
                 }
            }

                // Group key
            let group_email = attrs.get(&serde_yaml::Value::String("id".to_string()))
                .or_else(|| attrs.get(&serde_yaml::Value::String("email".to_string())))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{}@{}", group_name, customer_domain));

            builder = builder.add_block(hcl::Block::builder("group_key")
                .add_attribute(("id", group_email))
                .build());

                // Parent
                builder = builder.add_attribute(("parent", format!("customers/{}", customer_id)));

                // Labels
                let mut labels = hcl::Map::new();
                labels.insert("cloudidentity.googleapis.com/groups.discussion_forum".to_string(), hcl::Value::from(""));
                labels.insert("cloudidentity.googleapis.com/groups.security".to_string(), hcl::Value::from(""));
                builder = builder.add_attribute(("labels", hcl::Value::from(labels)));

                // Display Name & Description
                if let Some(dn) = attrs.get(&serde_yaml::Value::String("display_name".to_string())).and_then(|v| v.as_str()) {
                    builder = builder.add_attribute(("display_name", dn.to_owned()));
                } else {
                    builder = builder.add_attribute(("display_name", group_name.clone()));
                }

                if let Some(desc) = attrs.get(&serde_yaml::Value::String("description".to_string())).and_then(|v| v.as_str()) {
                    builder = builder.add_attribute(("description", desc.to_owned()));
                }

                // Initial Group Config
                let igc = attrs.get(&serde_yaml::Value::String("initial_group_config".to_string()))
                    .and_then(|v| v.as_str())
                    .unwrap_or("EMPTY");
                builder = builder.add_attribute(("initial_group_config", igc.to_owned()));

                blocks.push(builder.build());

                // Generate Import Block if requested
                if let Some(id) = attrs.get(&serde_yaml::Value::String("import-id".to_string())).and_then(|v| v.as_str()) {
                    import_blocks.push(hcl::Block::builder("import")
                        .add_attribute(("to", self.parse_hcl_expr(&format!("google_cloud_identity_group.{}", resource_name))))
                        .add_attribute(("id", id.to_string()))
                        .build());
                }

            // Handle Memberships - Aggregate roles by unique member email
            let group_ref = format!("google_cloud_identity_group.{}.id", resource_name);
            let role_types = [
                ("member", vec!["MEMBER"]),
                ("manager", vec!["MEMBER", "MANAGER"]),
                ("owner", vec!["MEMBER", "OWNER"]),
            ];

            let mut aggregated_members: std::collections::HashMap<String, std::collections::HashSet<String>> = std::collections::HashMap::new();

            for (key, roles) in role_types {
                if let Some(val) = attrs.get(&serde_yaml::Value::String(key.to_string())) {
                    let members_vals = match val {
                        serde_yaml::Value::Sequence(seq) => seq.clone(),
                        serde_yaml::Value::String(s) => vec![serde_yaml::Value::String(s.clone())],
                        _ => continue,
                    };

                    for member_val in members_vals {
                        if let Some(member_raw) = member_val.as_str() {
                            let entry = aggregated_members.entry(member_raw.to_string()).or_default();
                            for role in &roles {
                                entry.insert((*role).to_string());
                            }
                        }
                    }
                }
            }

            for (member_raw, roles_set) in aggregated_members {
                // Strip all prefixes (user:, group:, serviceAccount:)
                let member_email = if let Some(idx) = member_raw.find(':') {
                    &member_raw[idx + 1..]
                } else {
                    &member_raw
                };

                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                group_name.hash(&mut hasher);
                member_raw.hash(&mut hasher);
                let membership_label = format!("membership_{}_{:x}", resource_name, hasher.finish());

                let mut mb = hcl::Block::builder("resource")
                    .add_label("google_cloud_identity_group_membership")
                    .add_label(&membership_label);

                // Group reference
                mb = mb.add_attribute(hcl::Attribute::new("group", self.parse_hcl_expr(&group_ref)));

                // Member key
                mb = mb.add_block(hcl::Block::builder("preferred_member_key")
                    .add_attribute(("id", member_email.to_owned()))
                    .build());

                // Roles - uniquely sorted for stability
                let mut sorted_roles: Vec<_> = roles_set.into_iter().collect();
                sorted_roles.sort();
                for role in sorted_roles {
                    mb = mb.add_block(hcl::Block::builder("roles")
                        .add_attribute(("name", role))
                        .build());
                }

                if let Some(alias) = provider_alias {
                    if let Ok(expr) = (alias).parse::<hcl::Expression>() {
                        mb = mb.add_attribute(("provider", expr));
                    }
                }

                blocks.push(mb.build());
            }
        }
    }
}

    fn transpile_google_project_service(
        &self,
        blocks: &mut Vec<hcl::Block>,
        import_blocks: &mut Vec<hcl::Block>,
        project_ref: &str,
        service_val: &serde_yaml::Value,
        provider_alias: Option<&str>,
        safe_project_name: &str,
    ) {
        let service_configs = match service_val {
            serde_yaml::Value::String(s) => vec![(s.clone(), None)],
            serde_yaml::Value::Mapping(m) => {
                if let serde_yaml::Value::String(s) = m.get(&serde_yaml::Value::String("service".to_string())).unwrap_or(&serde_yaml::Value::Null) {
                    // Flat format: { service: "...", disable_on_destroy: ... }
                    vec![(s.clone(), Some(m))]
                } else {
                    // Nested format: { "service_name": { "disable_on_destroy": ... } }
                    let mut v = Vec::new();
                    for (mk, mv) in m {
                        if let serde_yaml::Value::String(ms) = mk {
                            v.push((ms.clone(), mv.as_mapping()));
                        }
                    }
                    v
                }
            }
            _ => return,
        };

        for (service, service_attrs) in service_configs {
            let safe_service = service.replace(".", "_");
            let label = format!("{}_{}", safe_project_name, safe_service);
            let project_expr = self.parse_hcl_expr(project_ref);
            let mut service_builder = hcl::Block::builder("resource")
                .add_label("google_project_service")
                .add_label(&label)
                .add_attribute(hcl::Attribute::new("project", project_expr))
                .add_attribute(("service", service.to_owned()));

            if let Some(alias) = provider_alias {
                if let Ok(expr) = alias.parse::<hcl::Expression>() {
                    service_builder = service_builder.add_attribute(("provider", expr));
                }
            }

            if let Some(attrs) = service_attrs {
                for (k, v) in attrs {
                    if let (serde_yaml::Value::String(k_str), Some(hcl_v)) = (k, self.yaml_to_hcl_value(v)) {
                        if k_str == "service" || k_str == "project" || k_str == "import-id" {
                            continue;
                        }
                        service_builder = service_builder.add_attribute(hcl::Attribute::new(k_str.clone(), hcl_v));
                    }
                }
            }

            blocks.push(service_builder.build());

            // Generate Import Block if requested
            if let Some(attrs) = service_attrs {
                if let Some(id) = attrs.get(&serde_yaml::Value::String("import-id".to_string())).and_then(|v| v.as_str()) {
                    import_blocks.push(hcl::Block::builder("import")
                        .add_attribute(("to", self.parse_hcl_expr(&format!("google_project_service.{}", label))))
                        .add_attribute(("id", id.to_string()))
                        .build());
                }
            }
        }
    }

    fn configure_google_provider(&self, mut builder: hcl::BlockBuilder, project_id: Option<String>, has_billing_project: bool, has_user_project_override: bool) -> hcl::BlockBuilder {
        // Use central infra project for billing/quota if available
        let infra_project = self.config.extra.get("infra-project-name").and_then(|v| v.as_str());

        if let Some(pid) = project_id {
            if !has_billing_project {
                let billing_pid = infra_project.unwrap_or(&pid);
                builder = builder.add_attribute(("billing_project", billing_pid.to_string()));
            }
            if !has_user_project_override {
                builder = builder.add_attribute(("user_project_override", true));
            }
        } else if let Some(infra_pid) = infra_project {
            // Even if no project_id is passed (e.g. default provider), use infra project for billing
            if !has_billing_project {
                builder = builder.add_attribute(("billing_project", infra_pid.to_string()));
            }
            if !has_user_project_override {
                builder = builder.add_attribute(("user_project_override", true));
            }
        }

        // Inject impersonation in cloud mode
        if self.get_deployment_mode() == "cloud" {
            if let (Some(account), Some(proj)) = (
                self.config.extra.get("svc-iac-account").and_then(|v| v.as_str()),
                self.config.extra.get("infra-project-name").and_then(|v| v.as_str())
            ) {
                let sa_email = format!("{}@{}.iam.gserviceaccount.com", account, proj);
                builder = builder.add_attribute(("impersonate_service_account", sa_email));
            }
        }
        builder
    }

    fn get_deployment_mode(&self) -> String {
        self.config.extra.get("deployment-mode")
            .and_then(|v| v.as_str())
            .unwrap_or("local")
            .to_string()
    }

    fn matches_pattern(&self, pattern: &str, text: &str) -> bool {
        if pattern.starts_with(".*") {
            text.ends_with(&pattern[2..])
        } else if pattern.ends_with(".*") {
            text.starts_with(&pattern[..pattern.len() - 2])
        } else {
            pattern == text
        }
    }
}
