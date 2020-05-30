.PHONY: download-platform-schema
download-platform-schema:
	echo '# GENERATED FILE – DO NOT EDIT' > aspen-cli/src/platform/schema.graphql
	npx get-graphql-schema https://platform.aspen-lang.com >> aspen-cli/src/platform/schema.graphql
