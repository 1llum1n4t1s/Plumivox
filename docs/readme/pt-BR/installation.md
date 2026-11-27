# Instalação

## Windows: instalar com o Setup (recomendado)

1. Baixe e execute o [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). A instalação é por usuário e não exige privilégios de administrador. O Setup é assinado com a assinatura de código do próprio autor (`Open Source Developer Yuichiro Shinozaki`).
2. Ele é instalado em `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` e, se as CLIs `claude` e `codex` estiverem no PATH, registra automaticamente o servidor MCP `plumivox` (argumento `--stdio`) no Claude Code (por usuário) e no Codex. Se as CLIs não forem encontradas, o registro é feito depois que você instalar a CLI, no próximo logon ou ao abrir o Plumivox pelo menu Iniciar.
3. Siga as instruções da tela exibida logo após a instalação e execute `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` no terminal para fazer a [configuração inicial](../../../README.pt-BR.md#configuração-inicial). O Claude Code e o Codex passam a usá-lo a partir de uma nova sessão.

As atualizações são automáticas. Enquanto o MCP está em execução, a nova versão é baixada do R2 (`https://plumivox-updates.kagayoi.com/velopack/`) e aplicada no logon do computador, somente quando o Plumivox (MCP e relay compartilhado) não estiver em execução. Se estiver em execução, a aplicação fica para o próximo logon, sem encerramento forçado. Antes de aplicar, o assinante é verificado; se for diferente, a atualização não é aplicada. Para desativar a atualização automática, defina `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` como `{"auto_update": false}`. O registro de funcionamento fica em `app-update.log`, na mesma pasta.

Para desinstalar, selecione o Plumivox na lista em "Configurações" → "Aplicativos". Somente os registros `plumivox` do Claude Code e do Codex que apontam para esta instalação e o registro de atualização no logon são removidos; a pasta de dados de configurações e sessões (`%LOCALAPPDATA%\Kagayoi\Plumivox`) é mantida. Para remover apenas os registros antes, confira o plano com `plumivox.exe --uninstall-setup` e execute com `--uninstall-setup --apply`.

Outras opções de linha de comando do `plumivox.exe`: `--setup` (configuração inicial), `--uninstall-setup` (remover registros) e `--rejection-log on|off|status|show|clear` (registro local por tempo limitado de textos de recusa com dados ocultados, desativado por padrão; veja [Registro local de recusas](when-posting-or-authentication-fails.md#registro-local-de-recusas)). O registro permanece após a desinstalação e é apagado com `--rejection-log clear`.

Ao migrar do plugin (abaixo): o Setup substitui o registro de mesmo nome `plumivox` que aponta para o `dist` do plugin antigo, entre outros, por um registro que aponta para o destino da instalação. O MCP fornecido pelo próprio plugin (`plumivox@plumivox-local`) é um registro separado; para que o Plumivox não seja iniciado por dois caminhos, desative-o ou remova o plugin. No Codex, adicione o mesmo `enabled = false` de [seção do Toolport](#usar-via-toolport) (ou remova o plugin); no Claude Code, desative ou desinstale o plugin em `/plugin`. O Codex dá preferência ao registro de mesmo nome em vez do MCP do plugin, então, enquanto o registro do Setup existir, deve iniciar apenas um, mas o Claude Code pode iniciar os dois. O Setup nunca altera as configurações do Codex nem do Claude Code; se o plugin continuar ativado, ele apenas avisa (no guia exibido ao iniciar o Plumivox, em `app-update.log` e como `legacyPlugins` em `app-update\setup-registration.json`). Os registros do Toolport também não são reescritos automaticamente; se você usa o Toolport, revise-o por conta própria conforme [seção do Toolport](#usar-via-toolport).

## Instalar como plugin (macOS, método tradicional)

No macOS (Apple Silicon), use este método até que a distribuição pelo Setup comece. No Windows ele também continua disponível.

A instalação é feita a partir do commit mais recente da branch `main` do repositório no GitHub. Não se usa o clone de desenvolvimento do usuário, nem se fixa uma tag ou SHA de commit.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` é o identificador do marketplace. A origem é a URL do GitHub acima. Como o `dist/plumivox.exe` para Windows x64 e o `dist/plumivox` (ARM64) para Mac com Apple Silicon já vêm incluídos, o usuário não precisa instalar dependências nem adicionar manualmente a configuração do MCP.

Na CI de desenvolvimento, os artefatos verificados no Windows e no macOS são armazenados no Cloudflare R2 e repassados durante a preparação da distribuição. O [procedimento de armazenamento e obtenção dos artefatos](../../distribution.md) é voltado a desenvolvedores. Usuários do plugin não precisam de configuração nem de credenciais do R2; a instalação e a atualização continuam sendo feitas a partir da `main` do GitHub.

Para atualizar, obtenha o estado mais recente da `main`, instale e reinicie o Codex para carregá-lo.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Confira o número da versão exibido em `installed` na lista. Para um plugin já instalado, não execute `add` novamente após a atualização; reinicie o Codex para carregar o novo MCP e as skills.

Para o registro e a atualização que acompanham o GitHub, consulte a [documentação de plugins do Codex](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

## Usar via Toolport

É possível registrar apenas o MCP no Toolport. Como o Plumivox não altera automaticamente o registro do Toolport, adicione ou altere o servidor stdio abaixo de acordo com o método de instalação. Não indique o clone de desenvolvimento nem o cache de instalação com número de versão.

- Nome: `plumivox`
- Comando: na versão Setup do Windows, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expanda `%LOCALAPPDATA%` para o caminho real; o caminho `current` não muda mesmo quando o conteúdo é substituído por uma atualização). Na versão plugin, confira a raiz obtida de `plumivox-local` com `codex plugin marketplace list` e use `<raiz obtida>/dist/plumivox.exe` no Windows ou `<raiz obtida>/dist/plumivox` no macOS
- Argumentos: `--stdio`
- Diretório de trabalho: o diretório pessoal (home) do usuário
- Tempo limite de requisição: 300000 milissegundos (inclui o tempo de login e de preparação do relay)

Ative-o no perfil do Toolport usado pelo Codex e confira as 11 ferramentas com `server="plumivox"` em `toolport_search_tools`. Busque e chame `list_accounts` e, depois de confirmar que a configuração foi carregada, desative a conexão direta. Na versão Setup, em uma instalação nova o registro direto é omitido automaticamente se o cliente já usa o gateway do Toolport e `plumivox` está habilitado no perfil usado (controlado por `mcp_registration` em `app-update.json`: `auto` por padrão, `direct` ou `off`; veja [Toolport configuration](../configuration.md)). Se você adicionar o Toolport depois, remova você mesmo os registros diretos existentes com `codex mcp remove plumivox` e `claude mcp remove plumivox --scope user`; o Setup nunca os remove por conta própria (depois que o registro é concluído, ele não é refeito automaticamente até você reinstalar com o Setup; só são retomados os clientes que o Setup omitiu, por exemplo se o gateway do Toolport sumir depois do cliente, na próxima vez que o Plumivox iniciar ou no próximo logon). A verificação é feita no Windows, no macOS e no Linux; em qualquer outro sistema, nada é verificado e o registro direto é sempre feito. Na versão plugin, adicione o seguinte ao `config.toml` das configurações de usuário do Codex, mantendo o plugin ativo para preservar a skill de configuração.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Ao reiniciar o Codex, a conexão direta fica desativada. A partir daí, busque as ferramentas no Toolport e passe o nome real da ferramenta retornado para `toolport_call_tool`. A chave de API, os cookies e as chaves SSH são lidos da configuração existente do Plumivox e não são passados como argumentos do Toolport.

A atualização da versão plugin é feita com o `codex plugin marketplace upgrade plumivox-local` mencionado acima. Como o servidor em execução no Toolport usa o código já carregado, após a atualização desative e reative o `plumivox` no Toolport para carregar o novo executável. Se a raiz obtida mudar, altere também o comando registrado. Nenhuma obtenção via Git ou instalação de dependências é feita na inicialização.
