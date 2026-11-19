<script lang="ts">
  import { onMount } from 'svelte';
  import { settings } from '$lib/stores/settings';

  onMount(() => {
    const unsubscribe = settings.subscribe(({ acrylic_transparency }) => {
      document.documentElement.style.setProperty('--glass-tint', `${100 - acrylic_transparency}%`);
    });
    const disableContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };

    document.addEventListener('contextmenu', disableContextMenu, { capture: true });

    return () => {
      unsubscribe();
      document.removeEventListener('contextmenu', disableContextMenu, { capture: true });
    };
  });
</script>

<slot />
