defmodule Kicksdk.NativeTest do
  # Changes the environment, so not async.
  use ExUnit.Case, async: false

  alias Kicksdk.Native.Build

  @targets RustlerPrecompiled.Config.default_targets()
  @no_env [get_env: &__MODULE__.no_env/1]

  def no_env(_var), do: nil

  test "a change to the build variables recompiles the NIF module" do
    refute Build.__mix_recompile__?()

    for var <- ~w(KICKSDK_BUILD KICKSDK_BASE_URL TARGET_ARCH) do
      before = System.get_env(var)

      try do
        System.put_env(var, "#{before}changed")
        assert Build.__mix_recompile__?()
      after
        if before, do: System.put_env(var, before), else: System.delete_env(var)
      end
    end

    refute Build.__mix_recompile__?()
  end

  test "Windows (architecture \"win32\") gets the precompiled NIF, not a source build" do
    # What rustler_precompiled itself decides for Windows.
    windows = %{os_type: {:win32, :nt}, target_system: %{}, word_size: 8, nif_version: "2.15"}
    assert {:ok, "nif-2.15-x86_64-pc-windows-msvc"} = RustlerPrecompiled.target(windows, @targets, ["2.15"])

    here = Build.here("2.15", [os_type: {:win32, :nt}, system_architecture: "win32", word_size: 8] ++ @no_env)
    assert here == windows
    assert Build.supported?(here, @targets, ["2.15"])
  end

  test "Unix targets: read from the architecture string or TARGET_*, else built from source" do
    linux = [os_type: {:unix, :linux}, system_architecture: "x86_64-pc-linux-gnu", word_size: 8]
    assert Build.supported?(Build.here("2.15", linux ++ @no_env), @targets, ["2.15"])

    mac = [os_type: {:unix, :darwin}, system_architecture: "aarch64-apple-darwin24.1.0", word_size: 8]
    assert Build.supported?(Build.here("2.15", mac ++ @no_env), @targets, ["2.15"])

    # Not three or four parts: cannot be read.
    odd = [os_type: {:unix, :linux}, system_architecture: "x86_64", word_size: 8]
    refute Build.supported?(Build.here("2.15", odd ++ @no_env), @targets, ["2.15"])

    # Nerves: TARGET_* on a Linux host, no vendor given, so "unknown".
    nerves = fn
      "TARGET_ARCH" -> "aarch64"
      "TARGET_OS" -> "linux"
      "TARGET_ABI" -> "gnu"
      _ -> nil
    end

    here = Build.here("2.15", linux ++ [get_env: nerves])
    assert here.target_system == %{arch: "aarch64", vendor: "unknown", os: "linux", abi: "gnu"}
    assert Build.supported?(here, @targets, ["2.15"])

    riscv32 = fn
      "TARGET_ARCH" -> "riscv32"
      _ -> nil
    end

    refute Build.supported?(Build.here("2.15", linux ++ [get_env: riscv32]), @targets, ["2.15"])
  end

  test "KICKSDK_BASE_URL works with or without a path" do
    default = "https://github.com/x/releases/download/v1"
    assert Build.base_url(nil, default) == default
    assert Build.base_url("http://localhost:8080", default) == "http://localhost:8080/"
    assert Build.base_url("http://localhost:8080/", default) == "http://localhost:8080/"
    assert Build.base_url("https://mirror/kicksdk/v1", default) == "https://mirror/kicksdk/v1"

    # As rustler_precompiled joins a file name to it.
    for base <- ["http://localhost:8080", "http://localhost:8080/"] do
      uri = URI.parse(Build.base_url(base, default))
      assert URI.to_string(%{uri | path: Path.join(uri.path, "f.tar.gz")}) == "http://localhost:8080/f.tar.gz"
    end
  end
end
