use anyhow::Result;
use keycloak::{KeycloakAdmin, KeycloakServiceAccountAdminTokenRetriever, types::*};
use reqwest;
pub struct KeycloakClient {
    admin: KeycloakAdmin<KeycloakServiceAccountAdminTokenRetriever>,
    realm: String,
}

impl KeycloakClient {
    pub async fn new(url: &str, realm: &str, client_id: &str, client_secret: &str) -> Result<Self> {
        // Create HTTP client
        let http_client = reqwest::Client::new();

        // Automatically acquires fresh tokens when needed
        let token_supplier = KeycloakServiceAccountAdminTokenRetriever::create_with_custom_realm(
            client_id,
            client_secret,
            realm,
            http_client.clone(),
        );

        let admin = KeycloakAdmin::new(url, token_supplier, http_client);
        let client = Self {
            admin,
            realm: realm.to_string(),
        };

        // Test the admin client by trying to get realm info
        match client.admin.realm_get(&client.realm).await {
            Ok(_) => tracing::info!("Keycloak admin client validated successfully"),
            Err(e) => tracing::warn!("Keycloak admin client validation failed: {:?}", e),
        }

        Ok(client)
    }

    pub async fn get_federated_identities(
        &self,
        user_id: &str,
    ) -> Result<Vec<FederatedIdentityRepresentation>> {
        Ok(self
            .admin
            .realm_users_with_user_id_federated_identity_get(&self.realm, user_id)
            .await?)
    }

    pub async fn delete_federated_identity(&self, user_id: &str, provider: &str) -> Result<()> {
        self.admin
            .realm_users_with_user_id_federated_identity_with_provider_delete(
                &self.realm,
                user_id,
                provider,
            )
            .await?;
        Ok(())
    }

    pub async fn get_user(&self, user_id: &str) -> Result<UserRepresentation> {
        Ok(self
            .admin
            .realm_users_with_user_id_get(&self.realm, user_id, None)
            .await?)
    }

    /// Helper to check if a specific Discord account is linked to a Keycloak user
    pub async fn get_discord_identity(
        &self,
        user_id: &str,
    ) -> Result<Option<FederatedIdentityRepresentation>> {
        let identities = self.get_federated_identities(user_id).await?;
        Ok(identities
            .into_iter()
            .find(|i| i.identity_provider.as_deref() == Some("discord")))
    }
}
