<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { invoke } from "@tauri-apps/api/core";

  const dispatch = createEventDispatcher();

  let currentStep = 1;
  let playerName = '';
  let gamesFolder = '~/Games/DeckDrop';
  let selectedTheme = 'light';
  let isConfiguring = false;
  let error = '';
  let showSteps = false;

  const themes = [
    { value: "light", label: "Light", icon: "☀️", description: "Clean and bright" },
    { value: "dark", label: "Dark", icon: "🌙", description: "Easy on the eyes" },
    { value: "cupcake", label: "Cupcake", icon: "🧁", description: "Sweet and playful" },
    { value: "bumblebee", label: "Bumblebee", icon: "🐝", description: "Warm and friendly" },
    { value: "emerald", label: "Emerald", icon: "💎", description: "Elegant and green" },
    { value: "synthwave", label: "Synthwave", icon: "🌆", description: "Retro futuristic" },
    { value: "cyberpunk", label: "Cyberpunk", icon: "🤖", description: "Neon and bold" },
    { value: "valentine", label: "Valentine", icon: "💝", description: "Romantic pink" },
    { value: "halloween", label: "Halloween", icon: "🎃", description: "Spooky orange" },
    { value: "garden", label: "Garden", icon: "🌱", description: "Natural and fresh" },
    { value: "forest", label: "Forest", icon: "🌲", description: "Deep and calming" },
    { value: "aqua", label: "Aqua", icon: "🌊", description: "Ocean blue" },
    { value: "lofi", label: "Lo-Fi", icon: "📻", description: "Vintage vibes" },
    { value: "pastel", label: "Pastel", icon: "🎨", description: "Soft and gentle" },
    { value: "fantasy", label: "Fantasy", icon: "🧚", description: "Magical and dreamy" },
    { value: "luxury", label: "Luxury", icon: "💎", description: "Premium and elegant" },
    { value: "dracula", label: "Dracula", icon: "🧛", description: "Dark and mysterious" },
    { value: "cmyk", label: "CMYK", icon: "🖨️", description: "Print-inspired" },
    { value: "autumn", label: "Autumn", icon: "🍂", description: "Warm and cozy" },
    { value: "coffee", label: "Coffee", icon: "☕", description: "Rich and warm" },
    { value: "winter", label: "Winter", icon: "❄️", description: "Cool and crisp" },
    { value: "nord", label: "Nord", icon: "🏔️", description: "Arctic blue" },
    { value: "sunset", label: "Sunset", icon: "🌅", description: "Golden hour" }
  ];

  function setTheme(theme: string) {
    selectedTheme = theme;
    document.documentElement.setAttribute('data-theme', theme);
  }

  function startSetup() {
    showSteps = true;
  }

  async function selectGamesFolder() {
    try {
      // TODO: Implement folder selection with Tauri dialog
      // For now, we'll use a simple input
      const newFolder = prompt('Enter games folder path:', gamesFolder);
      if (newFolder) {
        gamesFolder = newFolder;
      }
    } catch (err) {
      error = `Failed to select folder: ${err}`;
    }
  }

  async function completeSetup() {
    if (!playerName.trim()) {
      error = 'Please enter a player name';
      return;
    }

    if (!gamesFolder.trim()) {
      error = 'Please select a games folder';
      return;
    }

    isConfiguring = true;
    error = '';

    try {
      // Save configuration using Tauri command
      await invoke('save_initial_config', {
        playerName: playerName.trim(),
        gamesFolder: gamesFolder.trim()
      });
      
      // Save theme
      await invoke('update_theme', { theme: selectedTheme });
      
      dispatch('setupComplete');
    } catch (err) {
      error = `Failed to save configuration: ${err}`;
    } finally {
      isConfiguring = false;
    }
  }

  function nextStep() {
    if (currentStep < 4) {
      currentStep++;
    }
  }

  function prevStep() {
    if (currentStep > 1) {
      currentStep--;
    }
  }
</script>

<div class="max-w-2xl mx-auto">
  {#if !showSteps}
    <!-- Initial Welcome Screen -->
    <div class="text-center min-h-screen flex flex-col justify-center">
      <div class="w-24 h-24 bg-gradient-to-br from-primary to-secondary rounded-full flex items-center justify-center mx-auto mb-8 shadow-lg">
        <span class="text-4xl">🎮</span>
      </div>
      <h1 class="text-5xl font-bold bg-gradient-to-r from-primary to-secondary bg-clip-text text-transparent mb-6">
        Welcome to DeckDrop
      </h1>
      <div class="max-w-xl mx-auto mb-8">
        <p class="text-lg text-base-content/80 leading-relaxed mb-4">
          DeckDrop lets you distribute local non-Steam games in your local network. 
          Share your favorite games with friends and family on the same network.
        </p>
        <p class="text-base text-base-content/70 leading-relaxed">
          Get started with a quick setup that will only take a few minutes.
        </p>
      </div>
      <button class="btn btn-primary btn-lg px-8" on:click={startSetup}>
        Start Now
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7"></path>
        </svg>
      </button>
    </div>
  {:else}
    <!-- Setup Steps -->
    <!-- Progress Steps -->
    <div class="steps steps-horizontal w-full mb-8">
      <ul class="steps steps-horizontal w-full mb-8">
        <li data-content="👤" class="step {currentStep >= 1 ? 'step-info' : ''}" on:click={() => currentStep = 1}>
          Name
        </li>
        <li data-content="📁" class="step {currentStep >= 2 ? 'step-info' : ''}" on:click={() => currentStep = 2}>
          Folder
        </li>
        <li data-content="🎨" class="step {currentStep >= 3 ? 'step-info' : ''}" on:click={() => currentStep = 3}>
          Theme
        </li>
        <li data-content="✓" class="step {currentStep >= 4 ? 'step-info' : ''}" on:click={() => currentStep = 4}>
          Complete
        </li>
      </ul>
    </div>

    <!-- Error Alert -->
    {#if error}
      <div class="alert alert-error mb-6 shadow-lg">
        <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
        </svg>
        <span class="text-error-content">{error}</span>
      </div>
    {/if}

    <!-- Step Content -->
    <!-- Step 1: Player Name -->
    {#if currentStep === 1}
      <div class="card bg-base-100 shadow-xl border border-base-300/50">
        <div class="card-body">
          <h2 class="card-title text-2xl text-base-content mb-6">
            <div class="w-12 h-12 bg-gradient-to-br from-primary to-secondary rounded-lg flex items-center justify-center mr-4">
              <svg class="w-6 h-6 text-primary-content" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"></path>
              </svg>
            </div>
            Choose Your Name
          </h2>
          
          <div class="space-y-6">
            <p class="text-base-content/70 text-lg leading-relaxed">
              Other peers will see you under this name in the network. 
              Choose a name that identifies you.
            </p>
            
            <div class="form-control">
              <label class="label">
                <span class="label-text text-base-content font-medium">Your Name</span>
              </label>
              <input 
                type="text" 
                placeholder="e.g. SteamDeck_User" 
                class="input input-bordered input-lg w-full focus:input-primary" 
                bind:value={playerName}
                maxlength="32"
              />
              <label class="label">
                <span class="label-text-alt text-base-content/60">Maximum 32 characters</span>
              </label>
            </div>
          </div>
          
          <div class="card-actions justify-end mt-8">
            <button class="btn btn-primary btn-lg" on:click={nextStep} disabled={!playerName.trim()}>
              Next
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7"></path>
              </svg>
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Step 2: Games Folder -->
    {#if currentStep === 2}
      <div class="card bg-base-100 shadow-xl border border-base-300/50">
        <div class="card-body">
          <h2 class="card-title text-2xl text-base-content mb-6">
            <div class="w-12 h-12 bg-gradient-to-br from-primary to-secondary rounded-lg flex items-center justify-center mr-4">
              <svg class="w-6 h-6 text-primary-content" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2H5a2 2 0 00-2-2z"></path>
              </svg>
            </div>
            Select Games Folder
          </h2>
          
          <div class="space-y-6">
            <p class="text-base-content/70 text-lg leading-relaxed">
              Choose the folder where your DeckDrop games will be stored. 
              All downloaded games and metadata will be saved here.
            </p>
            
            <div class="form-control">
              <label class="label">
                <span class="label-text text-base-content font-medium">Games Folder</span>
              </label>
              <div class="input-group">
                <input 
                  type="text" 
                  placeholder="~/Games/DeckDrop" 
                  class="input input-bordered input-lg flex-1 focus:input-primary" 
                  bind:value={gamesFolder}
                  readonly
                />
                <button class="btn btn-square btn-lg" on:click={selectGamesFolder}>
                  <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2H5a2 2 0 00-2-2z"></path>
                  </svg>
                </button>
              </div>
              <label class="label">
                <span class="label-text-alt text-base-content/60">Default: ~/Games/DeckDrop</span>
              </label>
            </div>
          </div>
          
          <div class="card-actions justify-end mt-8">
            <button class="btn btn-ghost btn-lg" on:click={prevStep}>
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7"></path>
              </svg>
              Back
            </button>
            <button class="btn btn-primary btn-lg" on:click={nextStep} disabled={!gamesFolder.trim()}>
              Next
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7"></path>
              </svg>
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Step 3: Theme Selection -->
    {#if currentStep === 3}
      <div class="card bg-base-100 shadow-xl border border-base-300/50">
        <div class="card-body">
          <h2 class="card-title text-2xl text-base-content mb-6">
            <div class="w-12 h-12 bg-gradient-to-br from-primary to-secondary rounded-lg flex items-center justify-center mr-4">
              <svg class="w-6 h-6 text-primary-content" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zM21 5a2 2 0 00-2-2h-4a2 2 0 00-2 2v12a4 4 0 004 4h4a2 2 0 002-2V5z"></path>
              </svg>
            </div>
            Choose Your Theme
          </h2>
          
          <div class="space-y-6">
            <p class="text-base-content/70 text-lg leading-relaxed">
              Pick a theme that matches your style. You can change this later anytime.
            </p>
            
            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {#each themes as theme}
                <button 
                  class="card bg-base-200 hover:bg-base-300 transition-all duration-200 border-2 {selectedTheme === theme.value ? 'border-primary shadow-lg' : 'border-transparent'}"
                  on:click={() => setTheme(theme.value)}
                >
                  <div class="card-body p-4">
                    <div class="flex items-center gap-3">
                      <span class="text-2xl">{theme.icon}</span>
                      <div class="flex-1">
                        <h3 class="font-semibold text-base-content">{theme.label}</h3>
                        <p class="text-sm text-base-content/60">{theme.description}</p>
                      </div>
                      {#if selectedTheme === theme.value}
                        <div class="badge badge-primary">Selected</div>
                      {/if}
                    </div>
                  </div>
                </button>
              {/each}
            </div>
          </div>
          
          <div class="card-actions justify-end mt-8">
            <button class="btn btn-ghost btn-lg" on:click={prevStep}>
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7"></path>
              </svg>
              Back
            </button>
            <button class="btn btn-primary btn-lg" on:click={nextStep}>
              Next
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7"></path>
              </svg>
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Step 4: Complete Setup -->
    {#if currentStep === 4}
      <div class="card bg-base-100 shadow-xl border border-base-300/50">
        <div class="card-body">
          <h2 class="card-title text-2xl text-base-content mb-6">
            <div class="w-12 h-12 bg-gradient-to-br from-primary to-secondary rounded-lg flex items-center justify-center mr-4">
              <svg class="w-6 h-6 text-primary-content" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path>
              </svg>
            </div>
            Save Configuration
          </h2>
          
          <div class="space-y-6">
            <p class="text-base-content/70 text-lg leading-relaxed">
              Review your settings and save the configuration.
            </p>
            
            <div class="bg-base-200 p-6 rounded-lg border border-base-300/50">
              <h3 class="font-semibold text-base-content mb-4 text-lg">Your Settings:</h3>
              <div class="space-y-3 text-sm">
                <div class="flex justify-between items-center">
                  <span class="text-base-content/70">Name:</span>
                  <span class="text-base-content font-mono bg-base-300 px-3 py-1 rounded">{playerName}</span>
                </div>
                <div class="flex justify-between items-center">
                  <span class="text-base-content/70">Games Folder:</span>
                  <span class="text-base-content font-mono bg-base-300 px-3 py-1 rounded">{gamesFolder}</span>
                </div>
                <div class="flex justify-between items-center">
                  <span class="text-base-content/70">Theme:</span>
                  <span class="text-base-content font-mono bg-base-300 px-3 py-1 rounded">{selectedTheme}</span>
                </div>
              </div>
            </div>
          </div>
          
          <div class="card-actions justify-end mt-8">
            <button class="btn btn-ghost btn-lg" on:click={prevStep}>
              <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7"></path>
              </svg>
              Back
            </button>
            <button 
              class="btn btn-primary btn-lg" 
              on:click={completeSetup} 
              disabled={isConfiguring}
            >
              {#if isConfiguring}
                <span class="loading loading-spinner loading-sm"></span>
                Saving...
              {:else}
                Save Configuration
              {/if}
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Legal Disclaimer -->
    <div class="text-center mt-16 pt-8 border-t border-base-300/30">
      <p class="text-xs text-base-content/50 leading-relaxed max-w-lg mx-auto">
        <strong>Legal Notice:</strong> Please only share games that you own, are free, open-source, or that you have created yourself. 
        Respect copyright laws and only distribute games you have the right to share.
      </p>
    </div>
  {/if}
</div> 