defmodule Kicksdk.MixProject do
  use Mix.Project

  @version "0.1.0"
  @source "https://github.com/AnisAbdellatif/kicksdk"

  def project do
    [
      app: :kicksdk,
      version: @version,
      elixir: "~> 1.18",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      description:
        "Kick (kick.com) without the I/O: requests, parsers, the realtime chat protocol and webhook verification, from one Rust core",
      package: package(),
      docs: [main: "Kicksdk", source_url: @source, source_ref: "v#{@version}"]
    ]
  end

  def application, do: [extra_applications: [:logger]]

  defp deps do
    [
      {:rustler, "~> 0.38.0", runtime: false},
      {:ex_doc, "~> 0.38", only: :dev, runtime: false}
    ]
  end

  defp package do
    [
      licenses: ["MIT", "Apache-2.0"],
      links: %{"GitHub" => @source},
      files: ~w(lib native/kicksdk_nif/src native/kicksdk_nif/Cargo.toml mix.exs README.md)
    ]
  end
end
