<script lang="ts">
  import type { Profile } from '../types';

  let { profile, size = 52, ring = false }: { profile: Profile; size?: number; ring?: boolean } = $props();
</script>

<span
  class="avatar"
  role="img"
  aria-label="Avatar de {profile.name}"
  style:width="{size}px"
  style:height="{size}px"
  style:font-size="{Math.round(size * 0.42)}px"
  style:background={profile.accentSoft}
  style:color={profile.accentInk}
  style:box-shadow={ring ? `0 0 0 ${Math.max(3, size / 30)}px var(--ground), 0 0 0 ${Math.max(6, size / 15)}px ${profile.accent}` : 'none'}
>
  {#if profile.avatar}
    <img src={profile.avatar} alt="" />
  {:else}
    {profile.name.slice(0, 1)}
  {/if}
</span>

<style>
  .avatar {
    border-radius: 50%; display: inline-flex; align-items: center; justify-content: center;
    font-weight: 600; flex-shrink: 0; overflow: hidden;
  }
  img { width: 100%; height: 100%; object-fit: cover; }
</style>
