import { ClientSecretCredential } from "@azure/identity";

const required = [
  "AZURE_TENANT_ID",
  "AZURE_API_CLIENT_ID",
  "AZURE_CLIENT_ID",
  "AZURE_CLIENT_SECRET",
];

for (const name of required) {
  if (!process.env[name]) {
    console.error(`missing required environment variable ${name}`);
    process.exit(2);
  }
}

const credential = new ClientSecretCredential(
  process.env.AZURE_TENANT_ID,
  process.env.AZURE_CLIENT_ID,
  process.env.AZURE_CLIENT_SECRET
);

const token = await credential.getToken(
  `api://${process.env.AZURE_API_CLIENT_ID}/.default`
);

if (!token?.token) {
  console.error("Azure Identity did not return an access token");
  process.exit(3);
}

console.log(token.token);
