#!/usr/bin/env bash
# Pushes a .nupkg to nuget.org with trusted publishing: a GitHub OIDC token is exchanged for a short-lived API key,
# which is used once and never written to a step output. The exchange is the one NuGet/login performs (audience
# https://www.nuget.org, POST {username, tokenType: ApiKey} to /api/v2/token); the action itself is not on the
# organisation's allowed-actions list, so release.yml (job publish-nuget) runs this script instead.
#
# Usage: nuget-push.sh PACKAGE.nupkg
# Environment: NUGET_USER (the nuget.org account that created the trusted-publishing policy), and the
# ACTIONS_ID_TOKEN_REQUEST_URL / ACTIONS_ID_TOKEN_REQUEST_TOKEN pair that id-token: write provides.
# NUGET_TOKEN_SERVICE_URL and NUGET_SOURCE override the endpoints (tests only).
set -euo pipefail

package="${1:?usage: nuget-push.sh PACKAGE.nupkg}"
[ -f "$package" ] || { echo "::error::package not found: $package"; exit 1; }
[ -n "${NUGET_USER:-}" ] || { echo "::error::NUGET_USER is not set"; exit 1; }
if [ -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ] || [ -z "${ACTIONS_ID_TOKEN_REQUEST_TOKEN:-}" ]; then
  echo "::error::GitHub OIDC is not available; the job needs permissions id-token: write"
  exit 1
fi
service="${NUGET_TOKEN_SERVICE_URL:-https://www.nuget.org/api/v2/token}"
source="${NUGET_SOURCE:-https://api.nuget.org/v3/index.json}"

oidc="$(curl -sS --fail-with-body -H "Authorization: Bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
  "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=https%3A%2F%2Fwww.nuget.org" | jq -r '.value // empty')" || {
  echo "::error::could not get an OIDC token from GitHub"; exit 1; }
[ -n "$oidc" ] || { echo "::error::GitHub returned no OIDC token"; exit 1; }
echo "::add-mask::$oidc"

body="$(jq -cn --arg user "$NUGET_USER" '{username: $user, tokenType: "ApiKey"}')"
response="$(curl -sS -w '\n%{http_code}' -X POST "$service" -H 'Content-Type: application/json' \
  -H "Authorization: Bearer $oidc" -H 'User-Agent: nuget/login-action' --data "$body")" || {
  echo "::error::token exchange with $service failed"; exit 1; }
status="${response##*$'\n'}"
response="${response%$'\n'*}"
if [ "$status" != 200 ]; then
  echo "::error::token exchange failed (HTTP $status) at $service; NUGET_USER must be the account that created the policy"
  exit 1
fi
key="$(printf '%s' "$response" | jq -r '.apiKey // empty')"
[ -n "$key" ] || { echo "::error::the token exchange returned no apiKey"; exit 1; }
echo "::add-mask::$key"

dotnet nuget push "$package" --api-key "$key" --source "$source" --skip-duplicate
echo "::notice::$(basename "$package") pushed to $source"
