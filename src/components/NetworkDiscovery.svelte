<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { RefreshCw, AlertCircle, Users, Search } from 'lucide-svelte';
  
  export let peers: any[] = [];
  export let loading: boolean = false;
  export let error: string = '';
  export let fetchPeers: () => void = () => {};
  
  let mdnsStatus: string = '';
  let showDebugInfo = false;
  
  // Auto-refresh peers every 5 seconds
  let refreshInterval: ReturnType<typeof setInterval>;
  
  onMount(() => {
    // Initial fetch
    fetchPeers();
    
    // Set up auto-refresh
    refreshInterval = setInterval(() => {
      fetchPeers();
    }, 5000);
    
    // Get mDNS status
    invoke<string>('get_mdns_status').then(status => {
      mdnsStatus = status;
    }).catch((err: any) => {
      console.error('Failed to get mDNS status:', err);
    });
    
    return () => {
      if (refreshInterval) {
        clearInterval(refreshInterval);
      }
    };
  });
</script>

<!-- Status Card -->
<div class="card bg-base-100 shadow-xl mb-6">
  <div class="card-body">
    <h2 class="card-title text-base-content">
      <div class="badge badge-lg badge-primary">Network Discovery</div>
    </h2>
    
    <div class="flex items-center gap-4">
      <div class="flex items-center gap-2">
        {#if loading}
          <span class="loading loading-spinner loading-sm text-warning"></span>
        {:else}
          <div class="w-3 h-3 rounded-full bg-success"></div>
        {/if}
        <span class="font-medium text-base-content">
          {loading ? 'Searching for peers...' : `Found ${peers.length} peer${peers.length !== 1 ? 's' : ''}`}
        </span>
      </div>
      
      <div class="card-actions justify-end">
        <button 
          on:click={() => showDebugInfo = !showDebugInfo} 
          class="btn btn-ghost btn-sm"
        >
          Debug
        </button>
        <button 
          on:click={fetchPeers} 
          disabled={loading} 
          class="btn btn-primary btn-sm"
        >
          <RefreshCw class="w-4 h-4" />
          Refresh
        </button>
      </div>
    </div>
  </div>
  
  <!-- Debug Info -->
  {#if showDebugInfo}
    <div class="card bg-base-200 shadow-xl mb-6">
      <div class="card-body">
        <h3 class="card-title text-base-content">Debug Information</h3>
        <div class="text-xs font-mono bg-base-300 p-3 rounded">
          <pre>{mdnsStatus}</pre>
        </div>
      </div>
    </div>
  {/if}
</div>

<!-- Error Alert -->
{#if error}
  <div class="alert alert-error mb-6">
    <AlertCircle class="w-6 h-6" />
    <span class="text-error-content">{error}</span>
  </div>
{/if}

<!-- Peers Section -->
<div class="card bg-base-100 shadow-xl">
  <div class="card-body">
    <h2 class="card-title text-base-content">
      <Users class="w-6 h-6" />
      Discovered Peers
    </h2>
    
    {#if peers.length === 0 && !loading}
      <div class="text-center py-12">
        <Search class="w-16 h-16 mx-auto mb-4 text-base-content/40" />
        <h3 class="text-lg font-semibold mb-2 text-base-content">No peers discovered yet</h3>
        <p class="text-sm text-base-content/70">Make sure other DeckDrop instances are running on the same network</p>
        <div class="mt-4 p-4 bg-base-200 rounded-lg">
          <p class="text-xs text-base-content/60">Debug Info:</p>
          <p class="text-xs text-base-content/60">• Check if mDNS is working on your network</p>
          <p class="text-xs text-base-content/60">• Try restarting both instances</p>
          <p class="text-xs text-base-content/60">• Check firewall settings</p>
        </div>
      </div>
    {:else}
      <div class="grid gap-4">
        {#each peers as peer (peer.id)}
          <div class="card bg-base-200 hover:bg-base-300 transition-colors">
            <div class="card-body p-4">
              <div class="flex items-center justify-between">
                <div>
                  <div class="font-semibold text-sm text-base-content">
                    {#if peer.player_name}
                      {peer.player_name}
                    {:else}
                      {peer.id.slice(0, 8)}...
                    {/if}
                  </div>
                  <div class="text-xs text-base-content/70 font-mono">
                    {peer.id.slice(0, 8)}...
                  </div>
                  {#if peer.addr}
                    <div class="text-xs text-base-content/50">{peer.addr}</div>
                  {:else}
                    <div class="text-xs text-base-content/50 italic">Unknown address</div>
                  {/if}
                </div>
                <div class="badge badge-success badge-sm">Online</div>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div> 