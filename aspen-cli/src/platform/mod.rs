use graphql_client::{GraphQLQuery, Response};
use reqwest::Client;
use url::Url;
use uuid::Uuid;

#[derive(GraphQLQuery)]
#[graphql(
schema_path = "src/platform/schema.graphql",
query_path = "src/platform/queries.graphql",
response_derives = "Debug"
)]
pub struct Me;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/platform/schema.graphql",
    query_path = "src/platform/queries.graphql",
    response_derives = "Debug"
)]
pub struct SignIn;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Debug)]
pub enum ClientError {
    Reqwest(reqwest::Error),
    GraphQL(Vec<graphql_client::Error>),
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Reqwest(e)
    }
}

pub struct PlatformClient {
    url: Url,
    client: Client,
}

impl PlatformClient {
    pub fn new(url: Url) -> Result<PlatformClient, ClientError> {
        Ok(PlatformClient {
            url,
            client: Client::builder().cookie_store(true).user_agent(APP_USER_AGENT).build()?,
        })
    }

    pub async fn query<Q: GraphQLQuery>(
        &self,
        variables: Q::Variables,
    ) -> Result<Q::ResponseData, ClientError> {
        let query_body = Q::build_query(variables);
        let response = self
            .client
            .post(self.url.clone())
            .json(&query_body)
            .send()
            .await?;
        let body: Response<Q::ResponseData> = response.json().await?;

        if let Some(data) = body.data {
            Ok(data)
        } else {
            Err(ClientError::GraphQL(body.errors.unwrap_or(vec![])))
        }
    }
}
