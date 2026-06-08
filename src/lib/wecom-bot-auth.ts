import WecomAIBotSDK from '@wecom/wecom-aibot-sdk'

const WECOM_REGISTRATION_SOURCE = 'POINTER_APP'

export interface WecomBotCredentials {
  botId: string
  secret: string
}

export async function createWecomBotViaQr(): Promise<WecomBotCredentials> {
  const bot = await WecomAIBotSDK.openBotInfoAuthWindow({
    source: WECOM_REGISTRATION_SOURCE
  })
  const botId = bot.botid?.trim()
  const secret = bot.secret?.trim()
  if (!botId || !secret) {
    throw new Error('企微授权成功但凭证为空')
  }
  return { botId, secret }
}
