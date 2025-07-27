<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { 
    Palette, 
    Sun, 
    Moon, 
    Cake, 
    Zap, 
    Gem, 
    Building2, 
    Monitor, 
    Bot, 
    Heart, 
    Skull, 
    Sprout, 
    TreePine, 
    Waves, 
    Radio, 
    Sparkles, 
    Crown, 
    Printer, 
    Leaf, 
    Coffee, 
    Snowflake, 
    Mountain, 
    Sunset 
  } from 'lucide-svelte';
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
    { value: "light", label: "Light", icon: Sun },
    { value: "dark", label: "Dark", icon: Moon },
    { value: "cupcake", label: "Cupcake", icon: Cake },
    { value: "bumblebee", label: "Bumblebee", icon: Zap },
    { value: "emerald", label: "Emerald", icon: Gem },
    { value: "corporate", label: "Corporate", icon: Building2 },
    { value: "synthwave", label: "Synthwave", icon: Monitor },
    { value: "cyberpunk", label: "Cyberpunk", icon: Bot },
    { value: "valentine", label: "Valentine", icon: Heart },
    { value: "halloween", label: "Halloween", icon: Skull },
    { value: "garden", label: "Garden", icon: Sprout },
    { value: "forest", label: "Forest", icon: TreePine },
    { value: "aqua", label: "Aqua", icon: Waves },
    { value: "lofi", label: "Lo-Fi", icon: Radio },
    { value: "pastel", label: "Pastel", icon: Palette },
    { value: "fantasy", label: "Fantasy", icon: Sparkles },
    { value: "luxury", label: "Luxury", icon: Crown },
    { value: "dracula", label: "Dracula", icon: Skull },
    { value: "cmyk", label: "CMYK", icon: Printer },
    { value: "autumn", label: "Autumn", icon: Leaf },
    { value: "coffee", label: "Coffee", icon: Coffee },
    { value: "winter", label: "Winter", icon: Snowflake },
    { value: "nord", label: "Nord", icon: Mountain },
    { value: "sunset", label: "Sunset", icon: Sunset }
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
            <Palette class="w-5 h-5" />
          </div>
          <ul class="dropdown-content z-[1] menu p-2 shadow-lg bg-base-100 rounded-box w-52 max-h-96 overflow-y-auto">
            {#each themes as theme}
              <li>
                <button 
                  class="flex items-center gap-2 {currentTheme === theme.value ? 'active' : ''}"
                  on:click={() => setTheme(theme.value)}
                >
                  <svelte:component this={theme.icon} class="w-5 h-5" />
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
