use std::fs;
use std::path::Path;

pub struct TemplateArgs {
    pub customer_id: String,
    pub shortname: String,
    pub billing_id: String,
    pub region: String,
    pub org_id: String,
    pub domain: String,
    pub project_id: String,
    pub bucket_id: String,
    pub iac_user: String,
}

pub fn generate_template(args: &TemplateArgs, output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let content = format!(r#"variables:
  infra-folder-name: &infra-folder-name "Infrastructure"
  infra-project-name: &infra-project-name "{project_id}"
  infra-bucket-name: &infra-bucket-name "{bucket_id}"
  customer-id: &customer-id {customer_id}
  customer-organization-id: &customer-organization-id "{org_id}"
  customer-domain: &customer-domain "{domain}"
  customer-longname: &customer-longname ""
  customer-shortname: &customer-shortname "{shortname}"
  svc-iac-account: &svc-iac-account svc-iac-001
  svc-iac-users-group: &svc-iac-users-group svc-iac-users
  billing-account-infra: &billing-account-infra "{billing_id}"
  deployment-engine: &deployment-engine tofu
  deployment-mode: &deployment-mode local # switch by command
  default-region: &default-region {region}
  default-zone: &default-zone {region}-a

terraform:
  backend:
    local:
      path: "terraform.tfstate"
    gcs:
      bucket: *infra-bucket-name
      prefix: "hcl/state"

providers:
  google:
    project: *infra-project-name
    region: *default-region
    alias: google
    user_project_override: true
    billing_project: *infra-project-name
  google-beta:
    project: *infra-project-name
    region: *default-region
    alias: google-beta
    user_project_override: true
    billing_project: *infra-project-name

cloud_identity_group:
  *svc-iac-users-group:
    display_name: Service Account IaC Users
    description: Service account users allowed to impersonate the IaC service account
    owner:
      - !format ["{{}}@{{}}.iam.gserviceaccount.com", *svc-iac-account, *infra-project-name]
    member:
      - user:{iac_user}

google_organization_iam_member:
  # service needs to be added to group admin role in workspace console
  !format ["serviceAccount:{{}}@{{}}.iam.gserviceaccount.com", *svc-iac-account, *infra-project-name]:
    - roles/billing.user
    - roles/billing.projectManager
    - roles/iam.organizationRoleAdmin
    - roles/orgpolicy.policyAdmin
    - roles/owner
    - roles/resourcemanager.folderAdmin
    - roles/resourcemanager.organizationAdmin
    - roles/resourcemanager.projectIamAdmin
    - roles/resourcemanager.projectCreator
    - roles/iam.serviceAccountAdmin
    - roles/serviceusage.serviceUsageAdmin
    - roles/serviceusage.serviceUsageConsumer

  !format ["group:{{}}@{{}}", *svc-iac-users-group, *customer-domain]:
    - roles/iam.serviceAccountTokenCreator
    - roles/iam.serviceAccountUser
    - roles/serviceusage.serviceUsageConsumer

google_billing_account_iam_member:
  billing_account_id: *billing-account-infra
  !format ["serviceAccount:{{}}@{{}}.iam.gserviceaccount.com", *svc-iac-account, *infra-project-name]:
    - roles/billing.admin

folder:
  infra_folder:
    display_name: *infra-folder-name
    project:
      infra:
        project_id: *infra-project-name
        billing_account: *billing-account-infra
        project_service:
          - cloudbilling.googleapis.com
          - cloudidentity.googleapis.com
          - cloudresourcemanager.googleapis.com
          - iam.googleapis.com
          - iamcredentials.googleapis.com
          - orgpolicy.googleapis.com
          - serviceusage.googleapis.com
          - essentialcontacts.googleapis.com

        google_storage_bucket:
          state:
            import-id: *infra-bucket-name
            name: *infra-bucket-name
            location: *default-region
            force_destroy: true
            public_access_prevention: enforced
            uniform_bucket_level_access: true
            lifecycle_rule:
              - action:
                  type: Delete
                condition:
                  num_newer_versions: 100
                  with_state: ARCHIVED
              - action:
                  type: Delete
                condition:
                  days_since_noncurrent_time: 365

        google_service_account:
          provisioner:
            account_id: *svc-iac-account
            display_name: Primary IaC Provisioner

"#,
    customer_id = args.customer_id,
    project_id = args.project_id,
    bucket_id = args.bucket_id,
    org_id = args.org_id,
    domain = args.domain,
    shortname = args.shortname,
    billing_id = args.billing_id,
    region = args.region,
    iac_user = args.iac_user,
    );

    fs::write(output_path, content)?;
    Ok(())
}
