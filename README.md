# Garlic 🧄 - the AI Vampire 🧛 Warding Tool

Garlic is used to ward off vampires. According to Steve Yegge, AI tools have a vampiric effect on us, draining us of energy and making us tired and exhausted. Not because they are not good at coding, or do not make us much more productive, but simply because we get dopamine for getting stuff done quicker, leading us to work longer and think harder. In short, we need to touch grass. Instead of going hard for 12 hours straight with our coding agent of choice and burning ourselves out to only create value for our employer, we should be mindful of the $/hr formula and consider a new balance. He estimates there are no more than 3-4 hours of good work that we can do in a day with all this uplift without burning our own candles a little too brightly. As someone quite sensitive to the effects of extended dopamine release on the mind and body, I tend to agree with him. So I created `garlic`, a CLI tool that helps you keep the draining to a minimum and maintain your own energy levels so we can continue to be healthy little worker bees for years to come.

The idea came from [this article by Steve Yegge](https://steve-yegge.medium.com/the-ai-vampire-eda6e4f07163).

## How does it work?

`garlic` plugs into your coding agent ecosystem using hooks. It tracks when you start your coding work for the day and keeps an eye on how long you have been Claudling. As you start reaching the 3-4 hour mark it gently gets your agent to nudge you to maybe consider a break or even calling it for the day. It is highly customizable so that you can configure exactly when, how often, and how aggressively it nudges you. It is all based on hooks calling the `garlic` CLI which maintains a session log in your home directory. This is used to estimate how much you have been working in the day and has nothing to do with your usage limits in your agent of choice. Just because our usage limit says Go Go Go, does not mean it is good for us!

## Okay, how do I set it up?

First you install `garlic` with `uv`:

```bash
uv tool install garlic
```

Then you run `garlic setup` to configure it. It will create the hooks needed to connect to your session and prompts across all your projects. A `.garlic.yaml` config file is created in your home directory to control how garlic is set up.

You can run `garlic status` at any time to see how long you have been Claudling today (resets at 2am by default but can be changed).

You can run `garlic ignore` to pause the tracking for the day so it will not annoy you for just this one day.

## Things I should know?

The outputs from the `garlic hook` command, which are run by the hooks, are hardcoded in the project so there is no risk of it being used to prompt inject anything other than a gentle nudge for the agent to pass on to the user. You can audit the outputs here in the project.

I built this with Claude, and the idea was to keep garlic performant and use the standard Python library as much as possible to prevent any supply chain risks being introduced.
