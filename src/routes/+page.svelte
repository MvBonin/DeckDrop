<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import WelcomeSetup from "../components/WelcomeSetup.svelte";
  import NetworkDiscovery from "../components/NetworkDiscovery.svelte";

  interface PeerInfo {
    id: string;
    addr: string | null;
  }

  let peers: PeerInfo[] = [];
  let loading = false;
  let error = "";
  let currentTheme = "light";
  let isFirstTime = true;

  const themes = [
    { value: "light", label: "Light", icon: "☀️" },
    { value: "dark", label: "Dark", icon: "🌙" },
    { value: "cupcake", label: "Cupcake", icon: "🧁" },
    { value: "bumblebee", label: "Bumblebee", icon: "🐝" },
    { value: "emerald", label: "Emerald", icon: "💎" },
    { value: "corporate", label: "Corporate", icon: "🏢" },
    { value: "synthwave", label: "Synthwave", icon: "🌆" },
    { value: "retro", label: "Retro", icon: "📺" },
    { value: "cyberpunk", label: "Cyberpunk", icon: "🤖" },
    { value: "valentine", label: "Valentine", icon: "💝" },
    { value: "halloween", label: "Halloween", icon: "🎃" },
    { value: "garden", label: "Garden", icon: "🌱" },
    { value: "forest", label: "Forest", icon: "🌲" },
    { value: "aqua", label: "Aqua", icon: "🌊" },
    { value: "lofi", label: "Lo-Fi", icon: "📻" },
    { value: "pastel", label: "Pastel", icon: "🎨" },
    { value: "fantasy", label: "Fantasy", icon: "🧚" },
    { value: "wireframe", label: "Wireframe", icon: "📐" },
    { value: "black", label: "Black", icon: "⚫" },
    { value: "luxury", label: "Luxury", icon: "💎" },
    { value: "dracula", label: "Dracula", icon: "🧛" },
    { value: "cmyk", label: "CMYK", icon: "🖨️" },
    { value: "autumn", label: "Autumn", icon: "🍂" },
    { value: "business", label: "Business", icon: "💼" },
    { value: "acid", label: "Acid", icon: "⚗️" },
    { value: "lemonade", label: "Lemonade", icon: "🍋" },
    { value: "night", label: "Night", icon: "🌃" },
    { value: "coffee", label: "Coffee", icon: "☕" },
    { value: "winter", label: "Winter", icon: "❄️" },
    { value: "dim", label: "Dim", icon: "🌆" },
    { value: "nord", label: "Nord", icon: "🏔️" },
    { value: "sunset", label: "Sunset", icon: "🌅" },
    { value: "caramellatte", label: "Caramel Latte", icon: "🥤" },
    { value: "abyss", label: "Abyss", icon: "🌌" },
    { value: "silk", label: "Silk", icon: "🧵" }
  ];

  function setTheme(theme: string) {
    currentTheme = theme;
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('theme', theme);
    
    // Save theme to backend
    invoke('update_theme', { theme }).catch(err => {
      console.error('Failed to save theme:', err);
    });
  }

  async function fetchPeers() {
    try {
      loading = true;
      error = "";
      peers = await invoke("get_discovered_peers");
    } catch (err) {
      error = `Failed to fetch peers: ${err}`;
      console.error("Error fetching peers:", err);
    } finally {
      loading = false;
    }
  }

  async function checkConfiguration() {
    try {
      // Try to get config - if it fails, we need first-time setup
      const config: any = await invoke('get_config');
      
      // If we get here, config exists and is valid
      isFirstTime = false;
      
      // Load saved theme
      if (config.ui?.theme) {
        setTheme(config.ui.theme);
      }
    } catch (err) {
      // Any error means we need first-time setup
      isFirstTime = true;
      console.log('Configuration not found or invalid, showing first-time setup:', err);
    }
  }

  function onSetupComplete() {
    isFirstTime = false;
    // Start normal operation
    fetchPeers();
  }

  onMount(() => {
    // Always start with first-time setup
    isFirstTime = true;
    
    // Check for existing configuration after a short delay
    setTimeout(async () => {
      try {
        await checkConfiguration();
        console.log('Configuration check completed, isFirstTime:', isFirstTime);
        
        // Only fetch peers if not in first-time setup
        if (!isFirstTime) {
          fetchPeers();
          const interval = setInterval(fetchPeers, 2000);
          return () => clearInterval(interval);
        }
      } catch (err) {
        console.error('Error during configuration check:', err);
        // Stay in first-time setup
        isFirstTime = true;
      }
    }, 100);
  });
</script>

<!-- Template starts here -->
<div class="min-h-screen bg-gradient-to-br from-base-200 to-base-300">
  <!-- Modern Navbar -->
  {#if !isFirstTime}
    <nav class="navbar bg-base-100/80 backdrop-blur-sm border-b border-base-300/50 sticky top-0 z-50">
      <div class="navbar-start">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 bg-gradient-to-br from-primary to-secondary rounded-lg flex items-center justify-center">
            <span class="text-primary-content font-bold text-sm">DD</span>
          </div>
          <div>
            <div class="font-bold text-base-content">DeckDrop</div>
            <div class="text-xs text-base-content/60">Peer-to-peer gaming</div>
          </div>
        </div>
      </div>
      
      <div class="navbar-end">
        <!-- Theme Switcher -->
        <div class="dropdown dropdown-end">
          <div tabindex="0" role="button" class="btn btn-ghost btn-circle">
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zM21 5a2 2 0 00-2-2h-4a2 2 0 00-2 2v12a4 4 0 004 4h4a2 2 0 002-2V5z"></path>
            </svg>
          </div>
          <ul class="dropdown-content z-[1] menu p-2 shadow-lg bg-base-100 rounded-box w-52 max-h-96 overflow-y-auto">
            {#each themes as theme}
              <li>
                <button 
                  class="flex items-center gap-2 {currentTheme === theme.value ? 'active' : ''}"
                  on:click={() => setTheme(theme.value)}
                >
                  <span class="text-lg">{theme.icon}</span>
                  <span class="text-base-content">{theme.label}</span>
                  {#if currentTheme === theme.value}
                    <span class="ml-auto text-base-content">✓</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        </div>
      </div>
    </nav>
  {/if}

  <!-- Main Content -->
  <div class="container mx-auto p-6 max-w-4xl">
    {#if isFirstTime}
      <WelcomeSetup on:setupComplete={onSetupComplete} />
    {:else}
      <NetworkDiscovery {peers} {loading} {error} {fetchPeers} />
    {/if}
  </div>
</div>
