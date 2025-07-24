<script lang="ts">
  export let peers: any[] = [];
  export let loading: boolean = false;
  export let error: string = '';
  export let fetchPeers: () => void = () => {};
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
          on:click={fetchPeers} 
          disabled={loading} 
          class="btn btn-primary btn-sm"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"></path>
          </svg>
          Refresh
        </button>
      </div>
    </div>
  </div>
</div>

<!-- Error Alert -->
{#if error}
  <div class="alert alert-error mb-6">
    <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
    </svg>
    <span class="text-error-content">{error}</span>
  </div>
{/if}

<!-- Peers Section -->
<div class="card bg-base-100 shadow-xl">
  <div class="card-body">
    <h2 class="card-title text-base-content">
      <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"></path>
      </svg>
      Discovered Peers
    </h2>
    
    {#if peers.length === 0 && !loading}
      <div class="text-center py-12">
        <div class="text-6xl mb-4">🔍</div>
        <h3 class="text-lg font-semibold mb-2 text-base-content">No peers discovered yet</h3>
        <p class="text-sm text-base-content/70">Make sure other DeckDrop instances are running on the same network</p>
      </div>
    {:else}
      <div class="grid gap-4">
        {#each peers as peer (peer.id)}
          <div class="card bg-base-200 hover:bg-base-300 transition-colors">
            <div class="card-body p-4">
              <div class="flex items-center justify-between">
                <div>
                  <div class="font-mono font-semibold text-sm text-base-content">
                    {peer.id.slice(0, 8)}...
                  </div>
                  {#if peer.addr}
                    <div class="text-xs text-base-content/70">{peer.addr}</div>
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