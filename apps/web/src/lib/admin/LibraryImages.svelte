<script lang="ts">
  import ImageManager from '../catalog/ImageManager.svelte';
  import type { Library } from './types';
  let {library,onclose,onchanged}:{library:Library;onclose:()=>void;onchanged:()=>Promise<void>}=$props();
</script>
<ImageManager title={library.name} {library} {onclose} {onchanged} />
