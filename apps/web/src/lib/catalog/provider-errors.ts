export function providerErrorText(code: string): string {
  const messages: Record<string,string> = {
    unauthorized: 'La clé API est refusée par le fournisseur.',
    missing_api_key: 'La clé API de ce fournisseur manque.',
    network_error: 'Connexion au fournisseur impossible.',
    provider_timeout: 'Le fournisseur a dépassé le délai de réponse.',
    extension_execution_trapped: 'Le module du fournisseur a échoué pendant son exécution.',
    extension_output_too_large: 'Le fournisseur a renvoyé trop de données.',
    extension_module_invalid: 'Le module installé est incompatible ; mettez cette extension à jour.',
    extension_failed: 'Le module installé a échoué ; vérifiez sa version.',
    not_found: 'Aucune image pour ce titre chez ce fournisseur.',
    parse_error: 'Réponse du fournisseur illisible ; mettez cette extension à jour.',
    provider_error: 'Le fournisseur a renvoyé une erreur.',
  };
  return messages[code] ?? code;
}
