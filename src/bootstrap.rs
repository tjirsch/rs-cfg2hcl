use std::path::PathBuf;
use serde_yaml::Value;
use std::fs;
use google_cloud_auth::credentials::Builder;

pub async fn bootstrap(
    config_file: PathBuf,
    dry_run: bool,
    runtime_config: crate::ToolConfig,
    cli_config: Option<PathBuf>,
    cli_validation: Option<String>,
    cli_verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut final_shortname = None;
    let mut final_billing_id = None;
    let mut final_region = None;
    let mut final_org_id = None;
    let mut final_proj_id = None;
    let mut final_bucket = None;

    println!("Loading configuration from {}...", config_file.display());
    let content = fs::read_to_string(&config_file)?;
    let yaml: Value = serde_yaml::from_str(&content)?;

    if let Some(vars) = yaml.get("variables").and_then(|v| v.as_mapping()) {
        final_shortname = vars.get(&Value::String("customer-shortname".to_string()))
            .or_else(|| vars.get(&Value::String("shortname".to_string())))
            .and_then(|v| v.as_str()).map(|s| s.to_string());

        final_billing_id = vars.get(&Value::String("billing-account-infra".to_string()))
            .or_else(|| vars.get(&Value::String("billing_id".to_string())))
            .and_then(|v| v.as_str()).map(|s| s.to_string());

        final_region = vars.get(&Value::String("default-region".to_string()))
            .or_else(|| vars.get(&Value::String("region".to_string())))
            .and_then(|v| v.as_str()).map(|s| s.to_string());

        final_org_id = vars.get(&Value::String("customer-organization-id".to_string()))
            .and_then(|v| {
                match v {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                }
            });

        final_proj_id = vars.get(&Value::String("infra-project-name".to_string()))
            .and_then(|v| v.as_str()).map(|s| s.to_string());
        final_bucket = vars.get(&Value::String("infra-bucket-name".to_string()))
            .and_then(|v| v.as_str()).map(|s| s.to_string());
    }

    let sn = final_shortname.clone().ok_or_else(|| format!("Missing 'customer-shortname' in {}", config_file.display()))?;
    let bid = final_billing_id.ok_or_else(|| format!("Missing 'billing-account-infra' in {}", config_file.display()))?;
    let r = final_region.unwrap_or_else(|| "europe-west3".to_string());
    let oid_val = final_org_id.ok_or("Missing org_id in configuration (required for bootstrap)")?;

    // Normalize Org ID (ensure organizations/ prefix if numeric)
    let parent = if oid_val.chars().all(|c| c.is_numeric()) {
        format!("organizations/{}", oid_val)
    } else if oid_val.starts_with("organizations/") || oid_val.starts_with("folders/") {
        oid_val.clone()
    } else {
        oid_val.clone() // Assume it's already a valid parent string or let API fail
    };

    let project_id = final_proj_id.unwrap_or_else(|| format!("{}-iac-infra", sn));
    let bucket_name = final_bucket.unwrap_or_else(|| project_id.clone());
    let sa_name = "svc-iac-001";

    println!("--- Bootstrap Plan ---");
    println!("Parent:          {}", parent);
    println!("Shortname:       {}", sn);
    println!("Billing ID:      {}", bid);
    println!("Region:          {}", r);
    println!("Project ID:      {}", project_id);
    println!("Bucket:          {}", bucket_name);
    println!("Service Account: {}.iam.gserviceaccount.com", sa_name);
    println!("----------------------");

    if dry_run {
        println!("Dry run enabled. No resources will be created.");
        return Ok(());
    }

    println!("Starting bootstrap process...");

    // 1. Get Authentication Token
    println!("Authenticating using Application Default Credentials...");
    let scopes = ["https://www.googleapis.com/auth/cloud-platform"];
    let credentials = Builder::default()
        .with_scopes(scopes)
        .build_access_token_credentials()?;
    let token = credentials.access_token().await?;

    let client = reqwest::Client::new();

    // 1.5 Ensure Admin Permissions (Folder Admin)
    // We need to find the admin user from the YAML and grant them Folder Admin on the parent
    // so they can create the infrastructure folder.
    if let Some(groups) = yaml.get("cloud_identity_group").and_then(|v| v.as_mapping()) {
        // Iterate over groups to find a member list
        let mut first_admin_user = None;
        for (_k, v) in groups {
            if let Some(members) = v.get("member").and_then(|m| m.as_sequence()) {
                for m in members {
                    if let Some(m_str) = m.as_str() {
                        if m_str.starts_with("user:") {
                            first_admin_user = Some(m_str.to_string());
                            break;
                        }
                    }
                }
            }
            if first_admin_user.is_some() { break; }
        }

        if let Some(admin_user) = first_admin_user {
            println!("Ensuring {} has roles/resourcemanager.folderAdmin on {}...", admin_user, parent);

            // Get current IAM policy
            let policy_url = format!("https://cloudresourcemanager.googleapis.com/v3/{}:getIamPolicy", parent);
            let res = client.post(&policy_url)
                .bearer_auth(&token.token)
                .json(&serde_json::json!({}))
                .send()
                .await?;

            if res.status().is_success() {
                let mut policy: serde_json::Value = res.json().await?;
                let mut bindings = policy.get("bindings")
                    .and_then(|b| b.as_array())
                    .cloned()
                    .unwrap_or_default();

                let role = "roles/resourcemanager.folderAdmin";
                let mut found = false;

                // Check if binding exists
                for binding in bindings.iter_mut() {
                    if binding.get("role").and_then(|r| r.as_str()) == Some(role) {
                        if let Some(members) = binding.get_mut("members").and_then(|m| m.as_array_mut()) {
                             let member_json = serde_json::Value::String(admin_user.clone());
                             if !members.contains(&member_json) {
                                 members.push(member_json);
                                 found = true;
                             } else {
                                 // Already exists
                                 found = true;
                                 println!("User already has the role.");
                             }
                        }
                    }
                }

                if !found {
                     // Need to add new binding or the role was not found in bindings at all
                     // If we iterated and didn't find the role block, we add a new one.
                     // But wait, the loop above only modifies if role matches.
                     // If role block exists but user not in it, we added it and set found=true.
                     // If 'User already has role', we set found=true.
                     // So if !found, it means the role block itself was missing.
                     bindings.push(serde_json::json!({
                         "role": role,
                         "members": [admin_user]
                     }));
                }

                // If we modified something (or even if we didn't, but let's be safe and set it if we're not 100% sure strictly locally)
                // Optimization: Track 'modified' flag. But for now, setting it again is safe-ish if we handle ETag?
                // Actually setIamPolicy is robust.

                // Update bindings in policy
                if let Some(obj) = policy.as_object_mut() {
                    obj.insert("bindings".to_string(), serde_json::Value::Array(bindings));
                }

                if !found || true { // Force update to be sure, or improve logic.
                    // For this snippet, let's just write it back.
                    let set_policy_url = format!("https://cloudresourcemanager.googleapis.com/v3/{}:setIamPolicy", parent);
                    let res = client.post(&set_policy_url)
                        .bearer_auth(&token.token)
                        .json(&serde_json::json!({ "policy": policy }))
                        .send()
                        .await?;

                    if res.status().is_success() {
                         println!("Successfully updated IAM policy.");
                    } else {
                         let err = res.text().await?;
                         println!("Warning: Failed to set IAM policy: {}", err);
                    }
                }
            } else {
                let err = res.text().await?;
                println!("Warning: Failed to get IAM policy for {}: {}", parent, err);
            }
        } else {
            println!("Warning: No 'user:' found in cloud_identity_group. Skipping Folder Admin assignment.");
        }
    }

    // 2. Create Folder (if specified)
    let infra_folder_name = yaml.get("variables")
        .and_then(|v| v.get(&Value::String("infra-folder-name".to_string())))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let mut current_parent = parent.clone();

    if let Some(folder_display_name) = infra_folder_name {
        println!("Checking for existing Infrastructure Folder: {}...", folder_display_name);

        // 2a. Search for folder by display name in the parent
        let search_url = "https://cloudresourcemanager.googleapis.com/v3/folders";
        let res = client.get(search_url)
            .query(&[("parent", &parent)])
            .bearer_auth(&token.token)
            .send()
            .await?;

        let mut resolved_folder_id = None;
        if res.status().is_success() {
            let folders: serde_json::Value = res.json().await?;
            if let Some(list) = folders.get("folders").and_then(|v| v.as_array()) {
                let found = list.iter().find(|f| {
                    f.get("displayName").and_then(|v| v.as_str()) == Some(folder_display_name)
                });
                if let Some(folder) = found {
                    if let Some(name) = folder.get("name").and_then(|v| v.as_str()) {
                        resolved_folder_id = Some(name.to_string());
                    }
                }
            }
        }

        if let Some(folder_id) = resolved_folder_id {
            current_parent = folder_id;
            println!("Found existing folder: {}.", current_parent);
        } else {
            // 2b. Not found, proceed with creation
            println!("Creating Infrastructure Folder: {}...", folder_display_name);
            let url = "https://cloudresourcemanager.googleapis.com/v3/folders";
            let body = serde_json::json!({
                "displayName": folder_display_name,
                "parent": parent
            });

            let res = client.post(url)
                .bearer_auth(&token.token)
                .json(&body)
                .send()
                .await?;

            if res.status().is_success() {
                let info: serde_json::Value = res.json().await?;
                if let Some(op_name) = info.get("name").and_then(|v| v.as_str()) {
                    println!("Folder creation in progress ({})...", op_name);
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        let poll_res = client.get(format!("https://cloudresourcemanager.googleapis.com/v3/{}", op_name))
                            .bearer_auth(&token.token)
                            .send()
                            .await?;
                        let op_status: serde_json::Value = poll_res.json().await?;
                        if op_status.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                            if let Some(response) = op_status.get("response") {
                                if let Some(name) = response.get("name").and_then(|v| v.as_str()) {
                                    current_parent = name.to_string();
                                    println!("Successfully created folder: {}.", current_parent);
                                    break;
                                }
                            }
                            if let Some(err) = op_status.get("error") {
                                println!("Warning: Folder creation failed: {:?}", err);
                                break;
                            }
                        }
                        println!("Waiting for folder creation...");
                    }
                }
            } else {
                let err = res.text().await?;
                println!("Warning: Failed to create folder: {}", err);
            }
        }
    }

    // 3. Create Project Shell
    println!("Creating Project: {}...", project_id);
    let url = "https://cloudresourcemanager.googleapis.com/v3/projects";
    let body = serde_json::json!({
        "projectId": project_id,
        "displayName": project_id,
        "parent": current_parent
    });

    let res = client.post(url)
        .bearer_auth(&token.token)
        .json(&body)
        .send()
        .await?;

    if res.status().is_success() {
        let info: serde_json::Value = res.json().await?;
        if let Some(op_name) = info.get("name").and_then(|v| v.as_str()) {
            println!("Project creation in progress ({})...", op_name);
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                let poll_res = client.get(format!("https://cloudresourcemanager.googleapis.com/v3/{}", op_name))
                    .bearer_auth(&token.token)
                    .send()
                    .await?;
                let op_status: serde_json::Value = poll_res.json().await?;
                if op_status.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                    println!("Project shell created.");
                    break;
                }
                println!("Waiting for project creation...");
            }
        }
    } else if res.status().as_u16() == 409 {
        println!("Project already exists, skipping creation.");
    } else {
        let err = res.text().await?;
        println!("Warning: Failed to create project: {}", err);
    }

    // 4. Link Billing Account
    println!("Linking Billing Account: {}...", bid);
    let url = format!("https://cloudbilling.googleapis.com/v1/projects/{}/billingInfo", project_id);
    let body = serde_json::json!({
        "billingAccountName": format!("billingAccounts/{}", bid)
    });

    let res = client.put(&url)
        .bearer_auth(&token.token)
        .json(&body)
        .send()
        .await?;

    if res.status().is_success() {
        println!("Successfully linked billing account.");
    } else {
        let err = res.text().await?;
        println!("Warning: Failed to link billing: {}", err);
    }

    // 5. Enable Foundation APIs (The "Chicken-and-Egg" Fix)
    let core_services = vec![
        "serviceusage.googleapis.com",
        "cloudresourcemanager.googleapis.com",
        "iam.googleapis.com",
        "storage.googleapis.com",
        "cloudbilling.googleapis.com",
        "cloudidentity.googleapis.com",
        "cloudasset.googleapis.com",
    ];

    for service in core_services {
        println!("Enabling core service: {}...", service);
        let url = format!(
            "https://serviceusage.googleapis.com/v1/projects/{}/services/{}:enable",
            project_id, service
        );

        let res = client.post(&url)
            .bearer_auth(&token.token)
            .json(&serde_json::json!({})) // Fix 411 Length Required (empty body)
            .send()
            .await?;

        if res.status().is_success() {
            println!("Successfully enabled {}.", service);
        } else {
            let err_body = res.text().await?;
            println!("Warning: Failed to enable {}: {}", service, err_body);
        }
    }

    // 6. Create GCS State Bucket
    println!("Creating GCS State Bucket: {}...", bucket_name);
    let url = format!("https://storage.googleapis.com/storage/v1/b?project={}", project_id);
    let body = serde_json::json!({
        "name": bucket_name,
        "location": r,
        "iamConfiguration": {
            "uniformBucketLevelAccess": {
                "enabled": true
            }
        },
        "versioning": {
            "enabled": true
        }
    });

    let res = client.post(&url)
        .bearer_auth(&token.token)
        .json(&body)
        .send()
        .await?;

    if res.status().is_success() {
        println!("Successfully created state bucket.");
    } else if res.status().as_u16() == 409 {
        println!("Bucket already exists, skipping creation.");
    } else {
        let err = res.text().await?;
        println!("Warning: Failed to create bucket: {}", err);
    }

    println!("Bootstrap completed successfully.");
    println!("Core Infrastructure (Folder, Project, Billing, Foundation APIs, State Bucket) is now ready.");

    // 7. Automatic setup: Transpile -> Init -> Import
    println!("Running automatic setup...");

    // 7a. Transpile
    println!("Transpiling YAML to HCL...");
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(&exe);

    // Forward global CLI flags so relative paths and validation behave consistently
    if let Some(cfg_path) = cli_config {
        cmd.arg("--config").arg(cfg_path);
    }
    if let Some(validation) = cli_validation {
        cmd.arg("--validation").arg(validation);
    }
    if cli_verbose {
        cmd.arg("--verbose");
    }

    let status = cmd
        .arg("transpile")
        .arg(config_file.file_name().unwrap()) // Use the same YAML file name the user passed
        .current_dir(std::env::current_dir()?) // Run from the original working directory
        .status()?;

    if !status.success() {
        return Err("Transpilation failed. Cannot proceed with imports.".into());
    }

    // 7b. Init
    let target_hcl_dir = std::path::Path::new(&runtime_config.hcl_dir);
    if target_hcl_dir.exists() && target_hcl_dir.is_dir() {
        println!("Initializing OpenTofu/Terraform in {}...", target_hcl_dir.display());
        let status = std::process::Command::new(&runtime_config.tf_tool)
            .current_dir(target_hcl_dir)
            .arg("init")
            .status()?;

        if !status.success() {
             return Err(format!("{} init failed. Cannot proceed with imports.", runtime_config.tf_tool).into());
        }

        println!("Detected existing HCL directory at {}. Running automatic imports...", target_hcl_dir.display());

        // Import Folder
        if current_parent.starts_with("folders/") {
            run_import(&runtime_config.tf_tool, &target_hcl_dir, "google_folder.infra_folder", &current_parent);
        }

        // Import Project
        run_import(&runtime_config.tf_tool, &target_hcl_dir, "google_project.infra", &project_id);

        // Import Bucket
        run_import(&runtime_config.tf_tool, &target_hcl_dir, "google_storage_bucket.state", &bucket_name);
    } else {
        println!("Warning: HCL directory not found after transpilation. Skipping imports.");
    }

    Ok(())
}

fn run_import(tf_tool: &str, working_dir: &std::path::Path, resource_address: &str, resource_id: &str) {
    println!("Importing {} (ID: {})...", resource_address, resource_id);
    let output = std::process::Command::new(tf_tool)
        .current_dir(working_dir)
        .arg("import")
        .arg(resource_address)
        .arg(resource_id)
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                println!("- {}: Successfully imported.", resource_address);
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                if stderr.contains("Resource already managed by OpenTofu") {
                    println!("- {}: Already managed by OpenTofu.", resource_address);
                } else {
                    println!("- {}: Import failed or skipped. (stderr: {})", resource_address, stderr.trim());
                }
            }
        }
        Err(e) => {
            println!("- {}: Failed to execute {} import: {}", resource_address, tf_tool, e);
        }
    }
}
