# Configuração inicial

Se for o primeiro uso, siga o [guia de configuração](../../setup.md). Estão incluídos assistentes de configuração nativos para Windows e macOS e um instalador estático em Rust para VPS Linux x86_64 e aarch64. Para outras CPUs, mantém-se um caminho compatível com Bash. Também é possível pedir ao Codex "configure o Plumivox" para receber orientação pela skill incluída `setup-plumivox`.

O arquivo de configuração fica fora do repositório. Novos usuários usam `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` no Windows e `~/Library/Application Support/Kagayoi/Plumivox/settings.json` no macOS. Usuários que já têm `~/dev/Secret/secrets.json` têm o `twitterapi_io` desse arquivo lido. Os `release_posting` e `blog_posting` do vava/blbl também são mantidos. Para usar outro local, defina `PLUMIVOX_SECRETS_FILE`. Não é necessário passar valores secretos na conversa nem nos argumentos do MCP.

Se o arquivo de configuração selecionado não puder ser lido, ocorre um erro, sem troca para outra configuração. Se você alterar o local de armazenamento, reinicie o Codex. A configuração inicial não sobrescreve chaves existentes, contas de mesmo nome nem configurações do relay compartilhado. Para alterar configurações existentes, consulte o [guia de configuração](../../setup.md).

[Primeira configuração](../../setup.md) · [Configuração de contas e cookies](../../configuration.md) · [Gerenciar o proxy com o Plumivox](../../managed-proxy.md)
