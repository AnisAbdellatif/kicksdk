defmodule Kicksdk.Native.Build do
  @moduledoc false

  # What Kicksdk.Native decides when it compiles: where the precompiled NIF
  # comes from, and whether to build it from source instead. Apart from
  # Kicksdk.Native so that the decision can be tested, and so that Mix can
  # always load the module that says when to recompile (__mix_recompile__?/0
  # below): Kicksdk.Native cannot be loaded when its NIF does not load (after
  # a cross build, the NIF is for the other machine), and Mix then skips its
  # check. Kicksdk.Native calls this module while it compiles, so recompiling
  # this one recompiles it.

  @env ~w(KICKSDK_BUILD KICKSDK_BASE_URL TARGET_ARCH TARGET_VENDOR TARGET_OS TARGET_ABI
          RUSTLER_PRECOMPILED_FORCE_BUILD_ALL)
  @checksum Path.expand("../../../checksum-Elixir.Kicksdk.Native.exs", __DIR__)
  @inputs {Enum.map(@env, &System.get_env/1), File.exists?(@checksum)}

  @doc false
  # Mix asks before reusing this module's compiled code (and so Kicksdk.Native's).
  def __mix_recompile__?, do: inputs() != @inputs

  @doc false
  def inputs, do: {Enum.map(@env, &System.get_env/1), File.exists?(@checksum)}

  @doc false
  def checksum, do: @checksum

  @keys [arch: "TARGET_ARCH", vendor: "TARGET_VENDOR", os: "TARGET_OS", abi: "TARGET_ABI"]

  @doc false
  # The machine as rustler_precompiled describes a target, TARGET_* applied
  # (its own helpers for this are private). An architecture string that is
  # not three or four parts (Windows' is "win32") gives no parts.
  def here(nif_version, opts \\ []) do
    get_env = Keyword.get(opts, :get_env, &System.get_env/1)
    os_type = Keyword.get_lazy(opts, :os_type, &:os.type/0)
    word_size = Keyword.get_lazy(opts, :word_size, fn -> :erlang.system_info(:wordsize) end)

    architecture =
      Keyword.get_lazy(opts, :system_architecture, fn ->
        List.to_string(:erlang.system_info(:system_architecture))
      end)

    parts = String.split(architecture, "-")

    detected =
      if length(parts) in [3, 4],
        do: Map.new(Enum.zip(Keyword.keys(@keys), parts)),
        else: %{}

    system =
      Enum.reduce(@keys, detected, fn {key, var}, acc ->
        case get_env.(var) do
          nil -> acc
          value -> Map.put(acc, key, value)
        end
      end)

    # As rustler_precompiled does: a Linux target set through TARGET_*
    # without a vendor is "unknown".
    system =
      if system != detected and system[:vendor] == detected[:vendor] and system[:os] == "linux",
        do: Map.put(system, :vendor, "unknown"),
        else: system

    %{os_type: os_type, target_system: system, word_size: word_size, nif_version: nif_version}
  end

  @doc false
  # Whether rustler_precompiled finds a precompiled NIF for `here`. On Unix
  # it reads the target from the architecture string, and one it could not
  # read leaves no arch or os: build from source then. On Windows the string
  # is just "win32", and rustler_precompiled fills in the target from the
  # word size itself.
  def supported?(%{os_type: os_type, target_system: system} = here, targets, nif_versions) do
    readable? =
      case os_type do
        {:unix, _} -> Map.has_key?(system, :arch) and Map.has_key?(system, :os)
        _ -> true
      end

    readable? and
      try do
        match?({:ok, _}, RustlerPrecompiled.target(here, targets, nif_versions))
      rescue
        _ -> false
      end
  end

  @doc false
  # The release's URL, or KICKSDK_BASE_URL's. rustler_precompiled joins the
  # file name to the URL's path, so a URL without one ("http://mirror:8080")
  # gets "/": it would otherwise ask for "http://mirror:8080kicksdk_nif-…".
  def base_url(nil, default), do: default

  def base_url(url, _default) when is_binary(url) do
    case URI.parse(url) do
      %URI{path: path} = uri when path in [nil, ""] -> URI.to_string(%{uri | path: "/"})
      _ -> url
    end
  end
end
